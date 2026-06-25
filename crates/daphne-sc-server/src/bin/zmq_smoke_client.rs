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
        Command::ReadAfeReg {
            afe_block,
            reg_address,
        } => request_envelope(
            pb::MessageTypeV2::Mt2ReadAfeRegReq as i32,
            pb::CmdReadAfeReg {
                id: 1,
                afe_block,
                reg_address,
            }
            .encode_to_vec(),
        ),
        Command::SetAfeReset { asserted } => request_envelope(
            pb::MessageTypeV2::Mt2SetAfeResetReq as i32,
            pb::CmdSetAfeReset {
                id: 1,
                reset_value: asserted,
            }
            .encode_to_vec(),
        ),
        Command::DoAfeReset => request_envelope(
            pb::MessageTypeV2::Mt2DoAfeResetReq as i32,
            pb::CmdDoAfeReset { id: 1 }.encode_to_vec(),
        ),
        Command::SetAfePower { enabled } => request_envelope(
            pb::MessageTypeV2::Mt2SetAfePowerstateReq as i32,
            pb::CmdSetAfePowerState {
                id: 1,
                power_state: enabled,
            }
            .encode_to_vec(),
        ),
        Command::WriteAfeFunction {
            afe_block,
            function,
            config_value,
        } => request_envelope(
            pb::MessageTypeV2::Mt2WriteAfeFunctionReq as i32,
            pb::CmdWriteAfeFunction {
                afe_block,
                function,
                config_value,
            }
            .encode_to_vec(),
        ),
        Command::ConfigureMin {
            biasctrl,
            vgain,
            offset,
            adc_resolution,
        } => request_envelope(
            pb::MessageTypeV2::Mt2ConfigureFeReq as i32,
            minimal_configure_request(biasctrl, vgain, offset, adc_resolution).encode_to_vec(),
        ),
        Command::AlignAfe => request_envelope(
            pb::MessageTypeV2::Mt2AlignAfeReq as i32,
            pb::CmdAlignAfEs::default().encode_to_vec(),
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
        if let Some(firmware) = resp.firmware {
            let pl_devices = firmware.pl_devices_present.join(",");
            println!(
                "  firmware loaded={} fpga_state={} overlay={} build_id={} pl_devices={}",
                firmware.loaded,
                firmware.fpga_manager_state,
                firmware.overlay_name,
                firmware.build_id,
                pl_devices
            );
        }
        if let Some(rpu) = resp.rpu {
            println!(
                "  rpu available={} running={} firmware={} heartbeat={} last_fault={}",
                rpu.available, rpu.running, rpu.firmware, rpu.heartbeat, rpu.last_fault
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
        Ok(pb::MessageTypeV2::Mt2ReadAfeRegResp) => {
            let resp = pb::CmdReadAfeRegResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdReadAfeRegResponse")?;
            println!(
                "READ_AFE_REG success={} afeBlock={} regAddress={} regValue=0x{:04x} message={}",
                resp.success, resp.afe_block, resp.reg_address, resp.reg_value, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2SetAfeResetResp) => {
            let resp = pb::CmdSetAfeResetResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdSetAfeResetResponse")?;
            println!(
                "SET_AFE_RESET success={} resetValue={} message={}",
                resp.success, resp.reset_value, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2DoAfeResetResp) => {
            let resp = pb::CmdDoAfeResetResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdDoAfeResetResponse")?;
            println!(
                "DO_AFE_RESET success={} message={}",
                resp.success, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2SetAfePowerstateResp) => {
            let resp = pb::CmdSetAfePowerStateResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdSetAfePowerStateResponse")?;
            println!(
                "SET_AFE_POWER success={} powerState={} message={}",
                resp.success, resp.power_state, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2WriteAfeFunctionResp) => {
            let resp = pb::CmdWriteAfeFunctionResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdWriteAfeFunctionResponse")?;
            println!(
                "WRITE_AFE_FUNCTION success={} afeBlock={} function={} configValue={} message={}",
                resp.success, resp.afe_block, resp.function, resp.config_value, resp.message
            );
        }
        Ok(pb::MessageTypeV2::Mt2ConfigureFeResp) => {
            let resp = pb::ConfigureResponse::decode(response_env.payload.as_slice())
                .context("decoding ConfigureResponse")?;
            println!("CONFIGURE_FE success={}", resp.success);
            print_message_preview(&resp.message, 24);
        }
        Ok(pb::MessageTypeV2::Mt2AlignAfeResp) => {
            let resp = pb::CmdAlignAfEsResponse::decode(response_env.payload.as_slice())
                .context("decoding CmdAlignAfEsResponse")?;
            println!(
                "ALIGN_AFE success={} delay={:?} bitslip={:?}",
                resp.success, resp.delay, resp.bitslip
            );
            print_message_preview(&resp.message, 24);
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

fn minimal_configure_request(
    biasctrl: u32,
    vgain: u32,
    offset: u32,
    adc_resolution: bool,
) -> pb::ConfigureRequest {
    let channels = (0..40)
        .map(|channel| pb::ChannelConfig {
            id: channel,
            trim: 0,
            offset,
            gain: 1,
        })
        .collect();
    let afes = (0..5)
        .map(|afe| pb::AfeConfig {
            id: afe,
            attenuators: vgain,
            v_bias: 0,
            adc: Some(pb::AdcConfig {
                resolution: adc_resolution,
                output_format: true,
                sb_first: false,
            }),
            pga: Some(pb::PgaConfig {
                lpf_cut_frequency: 4,
                integrator_disable: true,
                gain: false,
            }),
            lna: Some(pb::LnaConfig {
                clamp: 0,
                gain: 2,
                integrator_disable: true,
            }),
        })
        .collect();

    pb::ConfigureRequest {
        daphne_address: "127.0.0.1".to_string(),
        slot: 0,
        timeout_ms: 30_000,
        biasctrl,
        self_trigger_threshold: 0x0C,
        self_trigger_xcorr: 0x68,
        tp_conf: 0x0010_DB35,
        compensator: 0x00FF_FFFF_FFFF,
        inverters: 0x00FF_0000_0000,
        channels,
        afes,
        full_stream_channels: Vec::new(),
    }
}

fn print_message_preview(message: &str, max_lines: usize) {
    let lines = message.lines().collect::<Vec<_>>();
    for line in lines.iter().take(max_lines) {
        println!("  {line}");
    }
    if lines.len() > max_lines {
        println!("  ... ({} more lines)", lines.len() - max_lines);
    }
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
    ReadAfeReg {
        afe_block: u32,
        reg_address: u32,
    },
    SetAfeReset {
        asserted: bool,
    },
    DoAfeReset,
    SetAfePower {
        enabled: bool,
    },
    WriteAfeFunction {
        afe_block: u32,
        function: String,
        config_value: u32,
    },
    ConfigureMin {
        biasctrl: u32,
        vgain: u32,
        offset: u32,
        adc_resolution: bool,
    },
    AlignAfe,
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
            Some("read-afe-reg") => {
                if positionals.len() != 3 {
                    bail!("read-afe-reg requires: afeBlock regAddress");
                }
                Command::ReadAfeReg {
                    afe_block: parse_u32(&positionals[1])?,
                    reg_address: parse_u32(&positionals[2])?,
                }
            }
            Some("set-afe-reset") => {
                if positionals.len() != 2 {
                    bail!("set-afe-reset requires: true|false");
                }
                Command::SetAfeReset {
                    asserted: parse_bool(&positionals[1])?,
                }
            }
            Some("do-afe-reset") => {
                if positionals.len() != 1 {
                    bail!("do-afe-reset does not accept arguments");
                }
                Command::DoAfeReset
            }
            Some("set-afe-power") => {
                if positionals.len() != 2 {
                    bail!("set-afe-power requires: true|false");
                }
                Command::SetAfePower {
                    enabled: parse_bool(&positionals[1])?,
                }
            }
            Some("write-afe-function") => {
                if positionals.len() != 4 {
                    bail!("write-afe-function requires: afeBlock function configValue");
                }
                Command::WriteAfeFunction {
                    afe_block: parse_u32(&positionals[1])?,
                    function: positionals[2].clone(),
                    config_value: parse_u32(&positionals[3])?,
                }
            }
            Some("configure-min") => {
                if positionals.len() > 5 {
                    bail!("configure-min accepts at most: biasctrl vgain offset adc_resolution");
                }
                Command::ConfigureMin {
                    biasctrl: parse_optional_u32(&positionals, 1, 0)?,
                    vgain: parse_optional_u32(&positionals, 2, 1600)?,
                    offset: parse_optional_u32(&positionals, 3, 2275)?,
                    adc_resolution: parse_optional_bool(&positionals, 4, true)?,
                }
            }
            Some("align-afe") => Command::AlignAfe,
            Some(other) => bail!("unknown command: {other}"),
        };

        Ok(Self {
            endpoint,
            command,
            timeout_ms,
        })
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "1" | "true" | "TRUE" | "on" | "ON" => Ok(true),
        "0" | "false" | "FALSE" | "off" | "OFF" => Ok(false),
        _ => bail!("invalid boolean: {value}; expected true/false or 1/0"),
    }
}

fn parse_optional_bool(positionals: &[String], index: usize, default: bool) -> Result<bool> {
    positionals
        .get(index)
        .map(|value| parse_bool(value))
        .unwrap_or(Ok(default))
}

fn parse_optional_u32(positionals: &[String], index: usize, default: u32) -> Result<u32> {
    positionals
        .get(index)
        .map(|value| parse_u32(value))
        .unwrap_or(Ok(default))
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
    println!(
        "  zmq_smoke_client [--endpoint tcp://host:port] read-afe-reg <afeBlock> <regAddress>"
    );
    println!("  zmq_smoke_client [--endpoint tcp://host:port] set-afe-reset <true|false>");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] do-afe-reset");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] set-afe-power <true|false>");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] write-afe-function <afeBlock> <function> <configValue>");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] configure-min [biasctrl=0] [vgain=1600] [offset=2275] [adc_resolution=true]");
    println!("  zmq_smoke_client [--endpoint tcp://host:port] align-afe");
    println!("default endpoint: {DEFAULT_ENDPOINT}");
}
