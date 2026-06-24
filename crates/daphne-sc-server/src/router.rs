use crate::handlers;
use crate::pb;
use crate::preflight::{LinuxPreflight, PreflightProvider, PreflightStatus};
use crate::v2;
use anyhow::{Context, Result};
use daphne_sc_core::rpu::RpuAfeTransport;
use daphne_sc_core::transport::{route_message_type, CommandRoute, MessageTypeV2};
use prost::Message;

pub struct RouterOptions {
    pub sndhwm: i32,
    pub rcvhwm: i32,
    pub sndbuf: i32,
    pub immediate: bool,
    pub max_envelope_bytes: usize,
}

impl Default for RouterOptions {
    fn default() -> Self {
        Self {
            sndhwm: 20_000,
            rcvhwm: 20_000,
            sndbuf: 4 * 1024 * 1024,
            immediate: true,
            max_envelope_bytes: 4 * 1024 * 1024,
        }
    }
}

pub fn bind_router(endpoint: &str, options: &RouterOptions) -> Result<zmq::Socket> {
    let context = zmq::Context::new();
    let socket = context
        .socket(zmq::ROUTER)
        .context("creating ZMQ ROUTER socket")?;
    socket.set_linger(0).context("setting ZMQ_LINGER")?;
    socket
        .set_sndhwm(options.sndhwm)
        .context("setting ZMQ_SNDHWM")?;
    socket
        .set_rcvhwm(options.rcvhwm)
        .context("setting ZMQ_RCVHWM")?;
    socket
        .set_sndbuf(options.sndbuf)
        .context("setting ZMQ_SNDBUF")?;
    socket
        .set_immediate(options.immediate)
        .context("setting ZMQ_IMMEDIATE")?;
    socket
        .bind(endpoint)
        .with_context(|| format!("binding ZMQ ROUTER to {endpoint}"))?;
    Ok(socket)
}

pub fn run<T: RpuAfeTransport>(endpoint: &str, options: RouterOptions, rpu: &mut T) -> Result<()> {
    let socket = bind_router(endpoint, &options)?;
    let mut preflight = LinuxPreflight::default();
    println!("ZMQ ROUTER listening on {endpoint}");

    loop {
        let frames = socket
            .recv_multipart(0)
            .context("receiving ZMQ multipart")?;
        if frames.len() < 2 {
            continue;
        }

        let client_id = frames[0].clone();
        let Some(payload) = frames.last() else {
            continue;
        };
        if payload.len() > options.max_envelope_bytes {
            continue;
        }

        let preflight_status = if needs_preflight(payload) {
            preflight.check()
        } else {
            PreflightStatus::ready()
        };
        let Some(response) = handle_envelope(payload, rpu, &preflight_status) else {
            continue;
        };

        socket
            .send(client_id, zmq::SNDMORE)
            .context("sending ZMQ client identity")?;
        socket
            .send(response, 0)
            .context("sending ZMQ response envelope")?;
    }
}

pub fn handle_envelope<T: RpuAfeTransport>(
    payload: &[u8],
    rpu: &mut T,
    preflight: &PreflightStatus,
) -> Option<Vec<u8>> {
    let req = pb::ControlEnvelopeV2::decode(payload).ok()?;
    if req.version != 2 || req.dir != pb::Direction::DirRequest as i32 {
        return None;
    }

    let response_payload = handlers::handle_payload(req.r#type, &req.payload, rpu, preflight);
    Some(v2::encode(v2::make_response(&req, response_payload)))
}

fn needs_preflight(payload: &[u8]) -> bool {
    let Ok(req) = pb::ControlEnvelopeV2::decode(payload) else {
        return false;
    };
    if req.version != 2 || req.dir != pb::Direction::DirRequest as i32 {
        return false;
    }
    let Ok(message_type) = MessageTypeV2::try_from(req.r#type as u32) else {
        return false;
    };
    matches!(route_message_type(message_type), CommandRoute::RpuAfe)
        || matches!(message_type, MessageTypeV2::ReadSlowControlStatusReq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb;
    use crate::preflight::{PreflightCheck, PreflightStatus};
    use daphne_sc_core::rpu::FailClosedRpuTransport;

    #[test]
    fn afe_write_gets_typed_fail_closed_response() {
        let request_payload = pb::CmdWriteAfeReg {
            id: 7,
            afe_block: 1,
            reg_address: 3,
            reg_value: 0x1234,
        }
        .encode_to_vec();

        let env = pb::ControlEnvelopeV2 {
            version: 2,
            dir: pb::Direction::DirRequest as i32,
            r#type: pb::MessageTypeV2::Mt2WriteAfeRegReq as i32,
            payload: request_payload,
            task_id: 42,
            msg_id: 100,
            correl_id: 0,
            route: "test".to_string(),
            timestamp_ns: 0,
        };

        let mut rpu = FailClosedRpuTransport;
        let preflight = PreflightStatus::ready();
        let response_bytes = handle_envelope(&env.encode_to_vec(), &mut rpu, &preflight).unwrap();
        let response_env = pb::ControlEnvelopeV2::decode(response_bytes.as_slice()).unwrap();

        assert_eq!(response_env.version, 2);
        assert_eq!(response_env.dir, pb::Direction::DirResponse as i32);
        assert_eq!(
            response_env.r#type,
            pb::MessageTypeV2::Mt2WriteAfeRegResp as i32
        );
        assert_eq!(response_env.task_id, 42);
        assert_eq!(response_env.correl_id, 100);
        assert_eq!(response_env.route, "test");

        let response = pb::CmdWriteAfeRegResponse::decode(response_env.payload.as_slice()).unwrap();
        assert!(!response.success);
        assert!(response.message.contains("RPU AFE backend is unavailable"));
        assert_eq!(response.afe_block, 1);
        assert_eq!(response.reg_address, 3);
        assert_eq!(response.reg_value, 0);
    }

    #[test]
    fn afe_write_is_rejected_when_preflight_fails() {
        let request_payload = pb::CmdWriteAfeReg {
            id: 7,
            afe_block: 1,
            reg_address: 3,
            reg_value: 0x1234,
        }
        .encode_to_vec();

        let env = pb::ControlEnvelopeV2 {
            version: 2,
            dir: pb::Direction::DirRequest as i32,
            r#type: pb::MessageTypeV2::Mt2WriteAfeRegReq as i32,
            payload: request_payload,
            task_id: 42,
            msg_id: 100,
            correl_id: 0,
            route: "test".to_string(),
            timestamp_ns: 0,
        };

        let mut rpu = FailClosedRpuTransport;
        let preflight = PreflightStatus::new(vec![PreflightCheck::fail(
            "clockchip.service",
            "state=failed",
        )]);
        let response_bytes = handle_envelope(&env.encode_to_vec(), &mut rpu, &preflight).unwrap();
        let response_env = pb::ControlEnvelopeV2::decode(response_bytes.as_slice()).unwrap();

        let response = pb::CmdWriteAfeRegResponse::decode(response_env.payload.as_slice()).unwrap();
        assert!(!response.success);
        assert!(response.message.contains("preflight failed"));
        assert!(response.message.contains("clockchip.service"));
        assert_eq!(response.afe_block, 1);
        assert_eq!(response.reg_address, 3);
        assert_eq!(response.reg_value, 0);
    }

    #[test]
    fn preflight_is_only_required_for_afe_routes() {
        let afe_env = pb::ControlEnvelopeV2 {
            version: 2,
            dir: pb::Direction::DirRequest as i32,
            r#type: pb::MessageTypeV2::Mt2WriteAfeRegReq as i32,
            ..Default::default()
        };
        let test_env = pb::ControlEnvelopeV2 {
            version: 2,
            dir: pb::Direction::DirRequest as i32,
            r#type: pb::MessageTypeV2::Mt2ReadTestRegReq as i32,
            ..Default::default()
        };

        assert!(needs_preflight(&afe_env.encode_to_vec()));
        assert!(!needs_preflight(&test_env.encode_to_vec()));
    }

    #[test]
    fn non_request_envelope_is_ignored() {
        let env = pb::ControlEnvelopeV2 {
            version: 2,
            dir: pb::Direction::DirResponse as i32,
            r#type: pb::MessageTypeV2::Mt2WriteAfeRegReq as i32,
            ..Default::default()
        };

        let mut rpu = FailClosedRpuTransport;
        let preflight = PreflightStatus::ready();
        assert!(handle_envelope(&env.encode_to_vec(), &mut rpu, &preflight).is_none());
    }
}
