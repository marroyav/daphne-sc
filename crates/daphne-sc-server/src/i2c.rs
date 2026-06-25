use daphne_sc_core::clockchip::ClockChipBus;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_PEC: libc::c_ulong = 0x0708;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const I2C_SMBUS_READ: u8 = 1;
const I2C_SMBUS_WORD_DATA: u32 = 3;

#[repr(C)]
union I2cSmbusData {
    byte: u8,
    word: u16,
    block: [u8; 34],
}

#[repr(C)]
struct I2cSmbusIoctlData {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut I2cSmbusData,
}

pub struct LinuxI2cDevice {
    file: File,
    bus: u8,
    address: u16,
}

impl LinuxI2cDevice {
    pub fn open(bus: u8, address: u16) -> Result<Self, LinuxI2cError> {
        let path = i2c_dev_path(bus);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| LinuxI2cError::Open {
                path: path.display().to_string(),
                source,
            })?;

        // SAFETY: ioctl is called with a valid file descriptor for an i2c-dev node
        // and an integer 7-bit slave address as required by Linux I2C_SLAVE.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), I2C_SLAVE, address as libc::c_ulong) };
        if rc < 0 {
            return Err(LinuxI2cError::SelectSlave {
                bus,
                address,
                source: std::io::Error::last_os_error(),
            });
        }

        Ok(Self { file, bus, address })
    }

    pub fn bus(&self) -> u8 {
        self.bus
    }

    pub fn address(&self) -> u16 {
        self.address
    }

    pub fn set_pec(&self, enabled: bool) -> Result<(), LinuxI2cError> {
        let value = if enabled { 1 } else { 0 };
        // SAFETY: ioctl is called with a valid i2c-dev file descriptor and an
        // integer PEC enable flag as required by Linux I2C_PEC.
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_PEC, value) };
        if rc < 0 {
            return Err(LinuxI2cError::Pec {
                bus: self.bus,
                address: self.address,
                source: std::io::Error::last_os_error(),
            });
        }
        Ok(())
    }

    pub fn read_word_data(&mut self, register: u8) -> Result<u16, LinuxI2cError> {
        let mut data = I2cSmbusData { word: 0 };
        let mut args = I2cSmbusIoctlData {
            read_write: I2C_SMBUS_READ,
            command: register,
            size: I2C_SMBUS_WORD_DATA,
            data: &mut data,
        };

        // SAFETY: ioctl is called with a valid i2c-dev file descriptor and a
        // pointer to the Linux i2c_smbus_ioctl_data layout.
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_SMBUS, &mut args) };
        if rc < 0 {
            return Err(LinuxI2cError::Read {
                bus: self.bus,
                address: self.address,
                register,
                source: std::io::Error::last_os_error(),
            });
        }

        // SAFETY: the kernel wrote the word field for an I2C_SMBUS_WORD_DATA
        // read when ioctl returned success.
        Ok(unsafe { data.word })
    }
}

impl ClockChipBus for LinuxI2cDevice {
    type Error = LinuxI2cError;

    fn write_register(&mut self, register: u8, value: u8) -> Result<(), Self::Error> {
        self.file
            .write_all(&[register, value])
            .map_err(|source| LinuxI2cError::Write {
                bus: self.bus,
                address: self.address,
                register,
                source,
            })
    }

    fn read_register(&mut self, register: u8) -> Result<u8, Self::Error> {
        self.file
            .write_all(&[register])
            .map_err(|source| LinuxI2cError::Write {
                bus: self.bus,
                address: self.address,
                register,
                source,
            })?;

        let mut byte = [0_u8; 1];
        self.file
            .read_exact(&mut byte)
            .map_err(|source| LinuxI2cError::Read {
                bus: self.bus,
                address: self.address,
                register,
                source,
            })?;
        Ok(byte[0])
    }

    fn reset_delay(&mut self) {
        thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Debug)]
pub enum LinuxI2cError {
    Open {
        path: String,
        source: std::io::Error,
    },
    SelectSlave {
        bus: u8,
        address: u16,
        source: std::io::Error,
    },
    Pec {
        bus: u8,
        address: u16,
        source: std::io::Error,
    },
    Read {
        bus: u8,
        address: u16,
        register: u8,
        source: std::io::Error,
    },
    Write {
        bus: u8,
        address: u16,
        register: u8,
        source: std::io::Error,
    },
    NoCandidateBus {
        address: u16,
        attempted: Vec<u8>,
    },
}

impl fmt::Display for LinuxI2cError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => write!(f, "could not open {path}: {source}"),
            Self::SelectSlave {
                bus,
                address,
                source,
            } => write!(
                f,
                "could not select I2C slave 0x{address:02X} on bus {bus}: {source}"
            ),
            Self::Pec {
                bus,
                address,
                source,
            } => write!(
                f,
                "could not set PEC for I2C slave 0x{address:02X} on bus {bus}: {source}"
            ),
            Self::Read {
                bus,
                address,
                register,
                source,
            } => write!(
                f,
                "I2C read failed on bus {bus} addr 0x{address:02X} reg 0x{register:02X}: {source}"
            ),
            Self::Write {
                bus,
                address,
                register,
                source,
            } => write!(
                f,
                "I2C write failed on bus {bus} addr 0x{address:02X} reg 0x{register:02X}: {source}"
            ),
            Self::NoCandidateBus { address, attempted } => {
                write!(
                    f,
                    "could not find I2C bus for addr 0x{address:02X}; attempted {attempted:?}"
                )
            }
        }
    }
}

impl std::error::Error for LinuxI2cError {}

pub fn i2c_bus_candidates(configured: Option<u8>) -> Vec<u8> {
    if let Some(bus) = configured {
        return vec![bus];
    }

    let Ok(entries) = fs::read_dir("/dev") else {
        return Vec::new();
    };
    let mut buses = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| name.strip_prefix("i2c-").and_then(|bus| bus.parse().ok()))
        .collect::<Vec<u8>>();
    buses.sort_unstable();
    buses
}

pub fn open_on_first_responsive_bus(
    configured: Option<u8>,
    address: u16,
    probe_register: u8,
) -> Result<LinuxI2cDevice, LinuxI2cError> {
    let mut attempted = Vec::new();
    for bus in i2c_bus_candidates(configured) {
        attempted.push(bus);
        let Ok(mut device) = LinuxI2cDevice::open(bus, address) else {
            continue;
        };
        if device.read_register(probe_register).is_ok() {
            return Ok(device);
        }
    }

    Err(LinuxI2cError::NoCandidateBus { address, attempted })
}

fn i2c_dev_path(bus: u8) -> PathBuf {
    PathBuf::from(format!("/dev/i2c-{bus}"))
}
