use anyhow::Result;
use daphne_sc_core::rpu::{FailClosedRpuTransport, RpuAfeTransport};
use daphne_sc_core::{route_message_type, CommandRoute, MessageTypeV2};
use daphne_sc_server::router;

const DEFAULT_BIND_ENDPOINT: &str = "tcp://*:40002";

fn main() -> Result<()> {
    let mut rpu = FailClosedRpuTransport;
    let rpu_status = rpu.status().expect("fail-closed RPU status is infallible");

    println!("daphne-sc server scaffold");
    println!("default bind endpoint: {DEFAULT_BIND_ENDPOINT}");
    println!(
        "RPU AFE backend: available={} running={}",
        rpu_status.available, rpu_status.running
    );
    println!(
        "MT2_WRITE_AFE_REG_REQ route: {:?}",
        route_message_type(MessageTypeV2::WriteAfeRegReq)
    );
    println!(
        "AFE commands fail closed until an RPU transport replaces {:?}",
        CommandRoute::RpuAfe
    );

    let args = Args::parse();
    if args.bind_smoke {
        let _socket = router::bind_router(&args.endpoint, &router::RouterOptions::default())?;
        println!("ZMQ ROUTER bound to {}", args.endpoint);
        println!("bind smoke test complete; transport loop not started");
        return Ok(());
    }

    router::run(&args.endpoint, router::RouterOptions::default(), &mut rpu)
}

struct Args {
    endpoint: String,
    bind_smoke: bool,
}

impl Args {
    fn parse() -> Self {
        let mut endpoint = DEFAULT_BIND_ENDPOINT.to_string();
        let mut bind_smoke = false;

        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--bind-smoke" => bind_smoke = true,
                "--help" | "-h" => {
                    println!("usage: daphne-sc-server [--bind-smoke] [endpoint]");
                    println!("default endpoint: {DEFAULT_BIND_ENDPOINT}");
                    std::process::exit(0);
                }
                _ => endpoint = arg,
            }
        }

        Self {
            endpoint,
            bind_smoke,
        }
    }
}
