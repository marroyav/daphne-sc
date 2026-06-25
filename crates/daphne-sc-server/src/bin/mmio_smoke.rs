use anyhow::{bail, Context, Result};
use daphne_sc_core::afe::{AFE_COUNT, CHANNELS_PER_AFE};
use daphne_sc_core::afe_hw::{
    afe_control_offset, frontend_bitslip_offset, frontend_delay_offset,
    spy_buffer_frame_clock_offset, AFE_GLOBAL_CONTROL_OFFSET, BIAS_ENABLE_OFFSET,
    DAC_GAIN_BIAS_CONTROL_OFFSET, DAC_GAIN_BIAS_U50_OFFSET, DAC_GAIN_BIAS_U53_OFFSET,
    DAC_GAIN_BIAS_U5_OFFSET, FPGA_REGISTER_BASE, FRONTEND_CONTROL_OFFSET, FRONTEND_STATUS_OFFSET,
    FRONTEND_TRIGGER_OFFSET,
};
use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, RawFd};
use std::ptr;

const DEV_MEM: &str = "/dev/mem";
const ENDPOINT_CLOCK_CONTROL_OFFSET: u32 = 0x0400_0000;
const ENDPOINT_CLOCK_STATUS_OFFSET: u32 = 0x0400_0004;
const ENDPOINT_STATUS_OFFSET: u32 = 0x0400_000C;

fn main() -> Result<()> {
    let args = Args::parse()?;
    let mem = PhysMem::open()?;

    println!(
        "mmio_smoke: read-only base=0x{:08X} dev={}",
        args.base, DEV_MEM
    );
    read_common_registers(&mem, args.base)?;
    if args.include_spy {
        read_spy_registers(&mem, args.base)?;
    } else {
        println!("spy frame-clock reads skipped");
    }

    Ok(())
}

fn read_common_registers(mem: &PhysMem, base: u64) -> Result<()> {
    read_named(mem, base, "afe.global_control", AFE_GLOBAL_CONTROL_OFFSET)?;
    for afe_pl in 0..AFE_COUNT {
        read_named(
            mem,
            base,
            &format!("afe.control.{afe_pl}"),
            afe_control_offset(afe_pl).expect("AFE index is bounded"),
        )?;
    }

    read_named(
        mem,
        base,
        "dac.gain_bias.control",
        DAC_GAIN_BIAS_CONTROL_OFFSET,
    )?;
    read_named(mem, base, "dac.gain_bias.u50", DAC_GAIN_BIAS_U50_OFFSET)?;
    read_named(mem, base, "dac.gain_bias.u53", DAC_GAIN_BIAS_U53_OFFSET)?;
    read_named(mem, base, "dac.gain_bias.u5", DAC_GAIN_BIAS_U5_OFFSET)?;
    read_named(mem, base, "bias.enable", BIAS_ENABLE_OFFSET)?;

    read_named(mem, base, "frontend.control", FRONTEND_CONTROL_OFFSET)?;
    read_named(mem, base, "frontend.status", FRONTEND_STATUS_OFFSET)?;
    read_named(mem, base, "frontend.trigger", FRONTEND_TRIGGER_OFFSET)?;
    for afe_board in 0..AFE_COUNT {
        read_named(
            mem,
            base,
            &format!("frontend.delay.{afe_board}"),
            frontend_delay_offset(afe_board).expect("AFE index is bounded"),
        )?;
        read_named(
            mem,
            base,
            &format!("frontend.bitslip.{afe_board}"),
            frontend_bitslip_offset(afe_board).expect("AFE index is bounded"),
        )?;
    }

    read_named(
        mem,
        base,
        "endpoint.clock_control",
        ENDPOINT_CLOCK_CONTROL_OFFSET,
    )?;
    read_named(
        mem,
        base,
        "endpoint.clock_status",
        ENDPOINT_CLOCK_STATUS_OFFSET,
    )?;
    read_named(mem, base, "endpoint.status", ENDPOINT_STATUS_OFFSET)?;
    Ok(())
}

fn read_spy_registers(mem: &PhysMem, base: u64) -> Result<()> {
    for afe_board in 0..AFE_COUNT {
        read_named(
            mem,
            base,
            &format!("spy.fclk.{afe_board}.{CHANNELS_PER_AFE}"),
            spy_buffer_frame_clock_offset(afe_board, 0).expect("AFE index is bounded"),
        )?;
    }
    Ok(())
}

fn read_named(mem: &PhysMem, base: u64, name: &str, offset: u32) -> Result<()> {
    let physical = base + u64::from(offset);
    let value = mem.read_u32(physical)?;
    println!("{name:28} phys=0x{physical:08X} value=0x{value:08X}");
    Ok(())
}

struct PhysMem {
    file: std::fs::File,
    page_size: u64,
}

impl PhysMem {
    fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(DEV_MEM)
            .with_context(|| format!("opening {DEV_MEM}; run as root on the DAPHNE board"))?;
        Ok(Self {
            file,
            page_size: page_size()?,
        })
    }

    fn read_u32(&self, address: u64) -> Result<u32> {
        read_phys_u32(self.file.as_raw_fd(), self.page_size, address)
    }
}

fn read_phys_u32(fd: RawFd, page_size: u64, address: u64) -> Result<u32> {
    let page_mask = !(page_size - 1);
    let page_base = address & page_mask;
    let page_offset = usize::try_from(address - page_base)
        .with_context(|| format!("invalid page offset for 0x{address:08X}"))?;

    // SAFETY: mmap is called on /dev/mem with a page-aligned physical offset.
    // The mapping is read-only and unmapped before returning.
    let mapping = unsafe {
        libc::mmap(
            ptr::null_mut(),
            usize::try_from(page_size).context("page size overflow")?,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            page_base as libc::off_t,
        )
    };
    if mapping == libc::MAP_FAILED {
        bail!(
            "mmap 0x{page_base:08X}: {}",
            std::io::Error::last_os_error()
        );
    }

    // SAFETY: page_offset points inside the mapped page and four bytes are read
    // using volatile byte reads to preserve MMIO semantics without assuming
    // alignment.
    let value = unsafe {
        let base = mapping.cast::<u8>().add(page_offset);
        let bytes = [
            ptr::read_volatile(base),
            ptr::read_volatile(base.add(1)),
            ptr::read_volatile(base.add(2)),
            ptr::read_volatile(base.add(3)),
        ];
        u32::from_le_bytes(bytes)
    };

    // SAFETY: mapping and length match the successful mmap call above.
    let rc = unsafe { libc::munmap(mapping, page_size as usize) };
    if rc != 0 {
        bail!("munmap: {}", std::io::Error::last_os_error());
    }

    Ok(value)
}

fn page_size() -> Result<u64> {
    // SAFETY: sysconf with _SC_PAGESIZE has no side effects.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        bail!("sysconf(_SC_PAGESIZE): {}", std::io::Error::last_os_error());
    }
    u64::try_from(value).context("invalid page size")
}

struct Args {
    base: u64,
    include_spy: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut base = FPGA_REGISTER_BASE;
        let mut include_spy = true;
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--base" => {
                    let value = args.next().context("--base requires a value")?;
                    base = parse_u64(&value)?;
                }
                "--skip-spy" => include_spy = false,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => bail!("unknown argument: {arg}"),
            }
        }

        Ok(Self { base, include_spy })
    }
}

fn parse_u64(value: &str) -> Result<u64> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).with_context(|| format!("invalid hex value: {value}"))
    } else {
        value
            .parse()
            .with_context(|| format!("invalid integer value: {value}"))
    }
}

fn print_usage() {
    println!("usage: mmio_smoke [--base 0x80000000] [--skip-spy]");
    println!("read-only /dev/mem smoke test for DAPHNE FPGA MMIO registers");
}
