use anyhow::Result;
use daphne_sc_core::rpu::{FailClosedRpuTransport, RpuAfeTransport};
use daphne_sc_core::{route_message_type, CommandRoute, MessageTypeV2};
use daphne_sc_server::router;
use daphne_sc_server::rpmsg::RpmsgAfeTransport;
use std::time::Duration;

const DEFAULT_BIND_ENDPOINT: &str = "tcp://*:40002";
const DEFAULT_RPU_TIMEOUT_MS: u64 = 200;

fn main() -> Result<()> {
    let args = Args::parse();

    println!("daphne-sc server scaffold");
    println!("bind endpoint: {}", args.endpoint);
    println!(
        "MT2_WRITE_AFE_REG_REQ route: {:?}",
        route_message_type(MessageTypeV2::WriteAfeRegReq)
    );
    println!(
        "AFE commands fail closed until an RPU transport replaces {:?}",
        CommandRoute::RpuAfe
    );

    if args.bind_smoke {
        let _socket = router::bind_router(&args.endpoint, &router::RouterOptions::default())?;
        println!("ZMQ ROUTER bound to {}", args.endpoint);
        println!("bind smoke test complete; transport loop not started");
        return Ok(());
    }

    if let Some(path) = args.rpu_rpmsg {
        println!(
            "RPU AFE backend: RPMsg path={} timeout_ms={}",
            path, args.rpu_timeout_ms
        );
        let mut rpu = RpmsgAfeTransport::open(path, Duration::from_millis(args.rpu_timeout_ms))?;
        router::run(&args.endpoint, router::RouterOptions::default(), &mut rpu)
    } else {
        let mut rpu = FailClosedRpuTransport;
        let rpu_status = rpu.status().expect("fail-closed RPU status is infallible");
        println!(
            "RPU AFE backend: available={} running={}",
            rpu_status.available, rpu_status.running
        );
        router::run(&args.endpoint, router::RouterOptions::default(), &mut rpu)
    }
}

struct Args {
    endpoint: String,
    bind_smoke: bool,
    rpu_rpmsg: Option<String>,
    rpu_timeout_ms: u64,
}

impl Args {
    fn parse() -> Self {
        let mut endpoint =
            env_nonempty("DAPHNE_SC_BIND").unwrap_or_else(|| DEFAULT_BIND_ENDPOINT.to_string());
        let mut bind_smoke = false;
        let mut rpu_rpmsg = env_nonempty("DAPHNE_SC_RPU_RPMSG");
        let mut rpu_timeout_ms = env_nonempty("DAPHNE_SC_RPU_TIMEOUT_MS")
            .map(|value| parse_timeout_ms(&value, "DAPHNE_SC_RPU_TIMEOUT_MS"))
            .unwrap_or(DEFAULT_RPU_TIMEOUT_MS);

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bind-smoke" => bind_smoke = true,
                "--rpu-rpmsg" => {
                    rpu_rpmsg = args.next();
                    if rpu_rpmsg.is_none() {
                        eprintln!("--rpu-rpmsg requires a path");
                        std::process::exit(2);
                    }
                }
                "--rpu-timeout-ms" => {
                    let Some(value) = args.next() else {
                        eprintln!("--rpu-timeout-ms requires a value");
                        std::process::exit(2);
                    };
                    rpu_timeout_ms = parse_timeout_ms(&value, "--rpu-timeout-ms");
                }
                "--help" | "-h" => {
                    println!("usage: daphne-sc-server [--bind-smoke] [--rpu-rpmsg PATH] [--rpu-timeout-ms MS] [endpoint]");
                    println!("default endpoint: {DEFAULT_BIND_ENDPOINT}");
                    println!("env: DAPHNE_SC_BIND, DAPHNE_SC_RPU_RPMSG, DAPHNE_SC_RPU_TIMEOUT_MS");
                    std::process::exit(0);
                }
                _ => endpoint = arg,
            }
        }

        Self {
            endpoint,
            bind_smoke,
            rpu_rpmsg,
            rpu_timeout_ms,
        }
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_timeout_ms(value: &str, source: &str) -> u64 {
    value.parse().unwrap_or_else(|_| {
        eprintln!("invalid {source} value: {value}");
        std::process::exit(2);
    })
}
