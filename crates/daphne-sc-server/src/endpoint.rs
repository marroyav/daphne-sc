use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::ptr;

const DEV_MEM: &str = "/dev/mem";
const EP_BASE: u64 = 0x8400_0000;
const EP_CLK_CTRL: u64 = EP_BASE;
const EP_CLK_STAT: u64 = EP_BASE + 0x4;
const EP_STAT: u64 = EP_BASE + 0xC;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointRegisterStatus {
    pub clock_control: u32,
    pub clock_status: u32,
    pub endpoint_status: u32,
}

impl EndpointRegisterStatus {
    pub fn endpoint_clock_source(self) -> bool {
        bit_set(self.clock_control, 2)
    }

    pub fn mmcm0_locked(self) -> bool {
        bit_set(self.clock_status, 0)
    }

    pub fn mmcm1_locked(self) -> bool {
        bit_set(self.clock_status, 1)
    }

    pub fn endpoint_state(self) -> u8 {
        (self.endpoint_status & 0xF) as u8
    }

    pub fn timestamp_ok(self) -> bool {
        bit_set(self.endpoint_status, 4)
    }
}

pub fn read_endpoint_register_status() -> Result<EndpointRegisterStatus, String> {
    Ok(EndpointRegisterStatus {
        clock_control: read_phys_u32(EP_CLK_CTRL)?,
        clock_status: read_phys_u32(EP_CLK_STAT)?,
        endpoint_status: read_phys_u32(EP_STAT)?,
    })
}

fn read_phys_u32(address: u64) -> Result<u32, String> {
    let file = OpenOptions::new()
        .read(true)
        .open(DEV_MEM)
        .map_err(|err| format!("{DEV_MEM}: {err}"))?;
    let page_size = page_size()?;
    let page_mask = !(page_size - 1);
    let page_base = address & page_mask;
    let page_offset = usize::try_from(address - page_base)
        .map_err(|_| format!("invalid page offset for 0x{address:08X}"))?;

    // SAFETY: mmap is called on /dev/mem with a page-aligned physical offset.
    // The mapping is read-only and unmapped before returning.
    let mapping = unsafe {
        libc::mmap(
            ptr::null_mut(),
            usize::try_from(page_size).map_err(|_| "page size overflow".to_string())?,
            libc::PROT_READ,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            page_base as libc::off_t,
        )
    };
    if mapping == libc::MAP_FAILED {
        return Err(format!(
            "mmap 0x{page_base:08X}: {}",
            std::io::Error::last_os_error()
        ));
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
        return Err(format!("munmap: {}", std::io::Error::last_os_error()));
    }

    Ok(value)
}

fn page_size() -> Result<u64, String> {
    // SAFETY: sysconf with _SC_PAGESIZE has no side effects.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        return Err(format!(
            "sysconf(_SC_PAGESIZE): {}",
            std::io::Error::last_os_error()
        ));
    }
    u64::try_from(value).map_err(|_| format!("invalid page size: {value}"))
}

fn bit_set(value: u32, bit: u8) -> bool {
    value & (1_u32 << bit) != 0
}
