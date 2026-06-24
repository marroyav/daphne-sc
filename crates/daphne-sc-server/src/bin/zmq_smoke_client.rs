use anyhow::{bail, Context, Result};
use daphne_sc_server::pb;
use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ENDPOINT: &str = "tcp://127.0.0.1:40002";

fn main() -> Result<()> {
    let args = Args::parse()?;
    let request = match args.command {
        Command::ReadTestReg => request_envelope(
            pb::MessageTypeV2::Mt2ReadTestRegReq as i32,
            pb::TestRegRequest::default().encode_to_vec(),
        ),
        Command::Status => request_envelope(
            1000,
            pb::sc::SlowControlStatusRequest { level: 1 }.encode_to_vec(),
        ),
        Command::WriteAfeReg {
            afe_block,
            reg_address,
            reg_value,
        } => request_envelope(
            pb::MessageTypeV2::Mt2WriteAfeRegReq as i32,
            pb::CmdWriteAfeReg {
                id: 1,
                afe_block,
                reg_address,
                reg_value,
            }
            .encode_to_vec(),
        ),
    };

    let context = zmq::Context::new();
    let socket = context
        .socket(zmq::DEALER)
        .context("creating ZMQ DEALER socket")?;
    socket
        .set_identity(b"daphne-sc-smoke")
        .context("setting ZMQ identity")?;
    socket.set_linger(0).context("setting ZMQ linger")?;
    socket
        .set_rcvtimeo(args.timeout_ms)
        .context("setting ZMQ receive timeout")?;
    socket
        .set_sndtimeo(args.timeout_ms)
        .context("setting ZMQ send timeout")?;
    socket
        .connect(&args.endpoint)
        .with_context(|| format!("connecting to {}", args.endpoint))?;

    socket
        .send(request.encode_to_vec(), 0)
        .context("sending request envelope")?;

    let response_bytes = socket
        .recv_bytes(0)
        .context("receiving response envelope")?;
    let response_env = pb::ControlEnvelopeV2::decode(response_bytes.as_slice())
        .context("decoding response envelope")?;

    println!(
        "response envelope: type={} task_id={} correl_id={}",
        response_env.r#type, response_env.task_id, response_env.correl_id
    );

    if response_env.r#type == 1001 {
        let resp = pb::sc::SlowControlStatusResponse::decode(response_env.payload.as_slice())
            .context("decoding SlowControlStatusResponse")?;
        println!(
            "STATUS success={} message={} services={} i2c_buses={} temperatures={} rails={} errors={}",
            resp.success,
            resp.message,
            resp.services.len(),
            resp.i2c.len(),
            resp.temperatures.len(),
            resp.rails.len(),
            resp.errors.len()
        );
        if let Some(clock) = resp.clocks {
            println!(
                "  clocks endpoint_source={} mmcm0={} mmcm1={} endpoint_raw=0x{:08x}",
                clock.endpoint_clock_source_controlled,
                clock.mmcm0_locked,
                clock.mmcm1_locked,
                clock.raw_endpoint_status
            );
        }
        for bus in resp.i2c {
            let devices = bus
                .devices
                .iter()
                .map(|device| format!("0x{:02x}:{}", device.address, device.name))
                .collect::<Vec<_>>()
                .join(",");
            println!("  i2c {} {} [{}]", bus.bus, bus.path, devices);
        }
        for temp in resp.temperatures.iter().take(8) {
            println!("  temp {} {:.2} C", temp.name, temp.celsius);
        }
        for rail in resp.rails.iter().take(12) {
            println!(
                "  rail {} voltage={:.4} current={:.4} power={:.4} status={}",
                rail.name, rail.voltage_v, rail.current_a, rail.power_w, rail.status
            );
        }
        for service in resp.services {
            println!(
                "  service {} active={} state={}",
                service.name, service.active, service.state
            );
        }
        for err in resp.errors {
            println!("  error {err}");
        }
        return Ok(());
    }

    match pb::MessageTypeV2::try_from(response_env.r#type) {
        Ok(pb::MessageTypeV2::Mt2ReadTestRegResp) => {
            let resp = pb::TestRegResponse::decode(response_env.payload.as_slice())
                .context("decoding TestRegResponse")?;
            println!(
                "READ_TEST_REG value=0x{:08x} message={}",
                resp.value, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2WriteAfeRegResp) => {
            let resp = pb::CmdWriteAfeRegResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdWriteAfeRegResponse")?;
            println!(
                "WRITE_AFE_REG success={} afeBlock={} regAddress={} regValue={} message={}",
                resp.success, resp.afe_block, resp.reg_address, resp.reg_value, resp.message
            );
        }
        Ok(other) => {
            println!(
                "unhandled response type {:?}; payload bytes={}",
                other,
                response_env.payload.len()
            );
        }
        Err(err) => {
            println!(
                "unknown response type {} ({err}); payload bytes={}",
                response_env.r#type,
                response_env.payload.len()
            );
        }
    }

    Ok(())
}

fn request_envelope(message_type: i32, payload: Vec<u8>) -> pb::ControlEnvelopeV2 {
    let msg_id = now_ns() & ((1_u64 << 63) - 1);
    pb::ControlEnvelopeV2 {
        version: 2,
        dir: pb::Direction::DirRequest as i32,
        r#type: message_type,
        payload,
        task_id: msg_id,
        msg_id,
        correl_id: 0,
        route: "smoke".to_string(),
        timestamp_ns: now_ns(),
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default()
}

enum Command {
    ReadTestReg,
    Status,
    WriteAfeReg {
        afe_block: u32,
        reg_address: u32,
        reg_value: u32,
    },
}

struct Args {
    endpoint: String,
    command: Command,
    timeout_ms: i32,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut endpoint = DEFAULT_ENDPOINT.to_string();
        let mut timeout_ms = 2_000;
        let mut positionals = Vec::new();

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--endpoint" => {
                    endpoint = args.next().context("--endpoint requires a value")?;
                }
                "--timeout-ms" => {
                    let value = args.next().context("--timeout-ms requires a value")?;
                    timeout_ms = value
                        .parse()
                        .with_context(|| format!("invalid --timeout-ms value: {value}"))?;
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => positionals.push(arg),
            }
        }

        let command = match positionals.first().map(String::as_str) {
            None | Some("read-test-reg") => Command::ReadTestReg,
            Some("status") => Command::Status,
            Some("write-afe-reg") => {
                if positionals.len() != 4 {
                    bail!("write-afe-reg requires: afeBlock regAddress regValue");
                }
                Command::WriteAfeReg {
                    afe_block: parse_u32(&positionals[1])?,
                    reg_address: parse_u32(&positionals[2])?,
                    reg_value: parse_u32(&positionals[3])?,
                }
            }
            Some(other) => bail!("unknown command: {other}"),
        };

        Ok(Self {
            endpoint,
            command,
            timeout_ms,
        })
    }
}

fn parse_u32(value: &str) -> Result<u32> {
    if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).with_context(|| format!("invalid hex integer: {value}"))
    } else {
        value
            .parse()
            .with_context(|| format!("invalid integer: {value}"))
    }
}

fn print_help() {
    println!("usage:");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] read-test-reg");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] status");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] write-afe-reg <afeBlock> <regAddress> <regValue>");
    println!("default endpoint: {DEFAULT_ENDPOINT}");
}
