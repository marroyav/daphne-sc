use crate::pb;
use prost::Message;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_MSG_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn response_type(req_type: i32) -> i32 {
    if req_type == 0 {
        0
    } else if req_type & 1 == 0 {
        req_type + 1
    } else {
        req_type
    }
}

pub fn make_response(req: &pb::ControlEnvelopeV2, payload: Vec<u8>) -> pb::ControlEnvelopeV2 {
    pb::ControlEnvelopeV2 {
        version: 2,
        dir: pb::Direction::DirResponse as i32,
        r#type: response_type(req.r#type),
        payload,
        task_id: req.task_id,
        msg_id: next_msg_id(),
        correl_id: req.msg_id,
        route: req.route.clone(),
        timestamp_ns: now_ns(),
    }
}

pub fn encode<M: Message>(message: M) -> Vec<u8> {
    message.encode_to_vec()
}

fn next_msg_id() -> u64 {
    let seq = NEXT_MSG_SEQ.fetch_add(1, Ordering::Relaxed);
    ((now_ns() << 16) ^ seq) & ((1_u64 << 63) - 1)
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default()
}
