use anyhow::{bail, Context, Result};
use daphne_sc_core::clockchip::{
    program_clock_chip, verify_clock_chip, ClockChipProgramOptions, ClockChipRange,
    CLOCKCHIP_DEFAULT_ADDR, CLOCKCHIP_REGISTERS, CLOCKCHIP_SANITY_REGISTER,
};
use daphne_sc_server::i2c::{i2c_bus_candidates, open_on_first_responsive_bus, LinuxI2cDevice};

fn main() -> Result<()> {
    let args = Args::parse()?;

    if args.dry_run {
        print_dry_run(&args);
        return Ok(());
    }

    let mut device = open_device(&args)?;
    match args.command {
        Command::Verify => {
            let report = verify_clock_chip(&mut device, &args.ranges)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            println!(
                "clockchip verify ok: bus={} addr=0x{:02X} verified={}",
                device.bus(),
                device.address(),
                report.verified
            );
        }
        Command::Program => {
            let options = ClockChipProgramOptions {
                verify: args.verify_after_write,
                reset: args.reset,
                ranges: args.ranges.clone(),
            };
            let report = program_clock_chip(&mut device, &options)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            println!(
                "clockchip program ok: bus={} addr=0x{:02X} written={} verified={} reset_pulses={}",
                device.bus(),
                device.address(),
                report.written,
                report.verified,
                report.reset_pulses
            );
        }
    }

    Ok(())
}

fn open_device(args: &Args) -> Result<LinuxI2cDevice> {
    if let Some(bus) = args.bus {
        return LinuxI2cDevice::open(bus, args.address).map_err(|err| anyhow::anyhow!("{err}"));
    }

    open_on_first_responsive_bus(None, args.address, CLOCKCHIP_SANITY_REGISTER)
        .map_err(|err| anyhow::anyhow!("{err}"))
}

fn print_dry_run(args: &Args) {
    let buses = args
        .bus
        .map(|bus| vec![bus])
        .unwrap_or_else(|| i2c_bus_candidates(None));
    println!(
        "dry-run: command={:?} candidate_buses={:?} addr=0x{:02X}",
        args.command, buses, args.address
    );
    for entry in CLOCKCHIP_REGISTERS {
        if args.ranges.is_empty()
            || args
                .ranges
                .iter()
                .any(|range| range.contains(entry.register))
        {
            println!("write 0x{:02X} <- 0x{:02X}", entry.register, entry.value);
        }
    }
    if matches!(args.command, Command::Program) && args.reset {
        println!("reset pulse register will be written after data table");
    }
}

#[derive(Debug, Clone, Copy)]
enum Command {
    Verify,
    Program,
}

struct Args {
    command: Command,
    bus: Option<u8>,
    address: u16,
    verify_after_write: bool,
    reset: bool,
    dry_run: bool,
    ranges: Vec<ClockChipRange>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut positionals = Vec::new();
        let mut bus = env_clockchip_bus()?;
        let mut address = env_clockchip_addr()?;
        let mut verify_after_write = false;
        let mut reset = true;
        let mut dry_run = false;
        let mut ranges = Vec::new();

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bus" => {
                    let value = args.next().context("--bus requires a value")?;
                    bus = Some(parse_u8(&value)?);
                }
                "--chip" | "--addr" => {
                    let value = args.next().context("--chip requires a value")?;
                    address = parse_u16(&value)?;
                }
                "--verify" => verify_after_write = true,
                "--no-reset" => reset = false,
                "--dry-run" => dry_run = true,
                "--only" => {
                    let value = args.next().context("--only requires a value")?;
                    ranges = parse_ranges(&value)?;
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => positionals.push(arg),
            }
        }

        let command = match positionals.first().map(String::as_str) {
            Some("verify") => Command::Verify,
            Some("program") => Command::Program,
            None => bail!("missing command: verify or program"),
            Some(other) => bail!("unknown command: {other}"),
        };

        Ok(Self {
            command,
            bus,
            address,
            verify_after_write,
            reset,
            dry_run,
            ranges,
        })
    }
}

fn parse_ranges(value: &str) -> Result<Vec<ClockChipRange>> {
    value
        .split(',')
        .filter(|token| !token.trim().is_empty())
        .map(|token| {
            let token = token.trim();
            if let Some((start, end)) = token.split_once('-') {
                Ok(ClockChipRange::new(parse_u8(start)?, parse_u8(end)?))
            } else {
                let register = parse_u8(token)?;
                Ok(ClockChipRange::new(register, register))
            }
        })
        .collect()
}

fn parse_u8(value: &str) -> Result<u8> {
    let parsed = parse_u16(value)?;
    u8::try_from(parsed).with_context(|| format!("value out of u8 range: {value}"))
}

fn parse_u16(value: &str) -> Result<u16> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16).with_context(|| format!("invalid hex integer: {value}"))
    } else {
        trimmed
            .parse()
            .with_context(|| format!("invalid integer: {value}"))
    }
}

fn print_help() {
    println!("usage:");
    println!("  clockchip_tool verify [--bus N] [--chip 0x70] [--only 0x1C-0x2A,0xE6]");
    println!(
        "  clockchip_tool program [--bus N] [--chip 0x70] [--verify] [--no-reset] [--dry-run]"
    );
    println!("default chip: 0x{CLOCKCHIP_DEFAULT_ADDR:02X}; no --bus means auto-discover");
    println!("env: CLOCKCHIP_BUS, CLOCKCHIP_ADDR");
}

fn env_clockchip_bus() -> Result<Option<u8>> {
    let Some(value) = std::env::var("CLOCKCHIP_BUS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    if value == "auto" {
        Ok(None)
    } else {
        Ok(Some(parse_u8(&value)?))
    }
}

fn env_clockchip_addr() -> Result<u16> {
    std::env::var("CLOCKCHIP_ADDR")
        .ok()
        .map(|value| parse_u16(&value))
        .transpose()
        .map(|value| value.unwrap_or(CLOCKCHIP_DEFAULT_ADDR))
}
