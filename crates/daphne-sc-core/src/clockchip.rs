use core::fmt;

pub const CLOCKCHIP_DEFAULT_ADDR: u16 = 0x70;
pub const CLOCKCHIP_DISCOVERY_ADDRS: &[u16] = &[0x70, 0x71, 0x72];
pub const CLOCKCHIP_SANITY_REGISTER: u8 = 0xE6;
pub const CLOCKCHIP_SANITY_VALUE: u8 = 0x06;
pub const CLOCKCHIP_RESET_REGISTER: u8 = 0xF6;
pub const CLOCKCHIP_RESET_SEQUENCE: &[u8] = &[0x02, 0x00];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockChipRegister {
    pub register: u8,
    pub value: u8,
}

pub const CLOCKCHIP_REGISTERS: &[ClockChipRegister] = &[
    reg(0x06, 0x08),
    reg(0x1C, 0x0B),
    reg(0x1D, 0x08),
    reg(0x1E, 0xB0),
    reg(0x1F, 0xC0),
    reg(0x20, 0xE3),
    reg(0x21, 0xE3),
    reg(0x22, 0xC0),
    reg(0x23, 0x41),
    reg(0x24, 0x06),
    reg(0x25, 0x00),
    reg(0x26, 0x00),
    reg(0x27, 0x06),
    reg(0x28, 0x64),
    reg(0x29, 0x0C),
    reg(0x2A, 0x24),
    reg(0x2D, 0x00),
    reg(0x2E, 0x00),
    reg(0x2F, 0x14),
    reg(0x30, 0x3A),
    reg(0x31, 0x00),
    reg(0x32, 0xC4),
    reg(0x33, 0x07),
    reg(0x34, 0x10),
    reg(0x35, 0x00),
    reg(0x36, 0x06),
    reg(0x37, 0x00),
    reg(0x38, 0x00),
    reg(0x39, 0x00),
    reg(0x3A, 0x00),
    reg(0x3B, 0x01),
    reg(0x3C, 0x00),
    reg(0x3D, 0x00),
    reg(0x3E, 0x00),
    reg(0x3F, 0x10),
    reg(0x40, 0x00),
    reg(0x41, 0x00),
    reg(0x42, 0x00),
    reg(0x43, 0x00),
    reg(0x44, 0x00),
    reg(0x45, 0x00),
    reg(0x46, 0x00),
    reg(0x47, 0x00),
    reg(0x48, 0x00),
    reg(0x49, 0x00),
    reg(0x4A, 0x10),
    reg(0x4B, 0x00),
    reg(0x4C, 0x00),
    reg(0x4D, 0x00),
    reg(0x4E, 0x00),
    reg(0x4F, 0x00),
    reg(0x50, 0x00),
    reg(0x51, 0x00),
    reg(0x52, 0x00),
    reg(0x53, 0x00),
    reg(0x54, 0x00),
    reg(0x55, 0x10),
    reg(0x56, 0x80),
    reg(0x57, 0x0A),
    reg(0x58, 0x00),
    reg(0x59, 0x00),
    reg(0x5A, 0x00),
    reg(0x5B, 0x00),
    reg(0x5C, 0x01),
    reg(0x5D, 0x00),
    reg(0x5E, 0x00),
    reg(0x5F, 0x00),
    reg(0x61, 0x00),
    reg(0x62, 0x30),
    reg(0x63, 0x00),
    reg(0x64, 0x00),
    reg(0x65, 0x00),
    reg(0x66, 0x00),
    reg(0x67, 0x01),
    reg(0x68, 0x00),
    reg(0x69, 0x00),
    reg(0x6A, 0x80),
    reg(0x6B, 0x00),
    reg(0x6C, 0x00),
    reg(0x6D, 0x00),
    reg(0x6E, 0x40),
    reg(0x6F, 0x00),
    reg(0x70, 0x00),
    reg(0x71, 0x00),
    reg(0x72, 0x40),
    reg(0x73, 0x00),
    reg(0x74, 0x80),
    reg(0x75, 0x00),
    reg(0x76, 0x40),
    reg(0x77, 0x00),
    reg(0x78, 0x00),
    reg(0x79, 0x00),
    reg(0x7A, 0x40),
    reg(0x7B, 0x00),
    reg(0x7C, 0x00),
    reg(0x7D, 0x00),
    reg(0x7E, 0x00),
    reg(0x7F, 0x00),
    reg(0x80, 0x00),
    reg(0x81, 0x00),
    reg(0x82, 0x00),
    reg(0x83, 0x00),
    reg(0x84, 0x00),
    reg(0x85, 0x00),
    reg(0x86, 0x00),
    reg(0x87, 0x00),
    reg(0x88, 0x00),
    reg(0x89, 0x00),
    reg(0x8A, 0x00),
    reg(0x8B, 0x00),
    reg(0x8C, 0x00),
    reg(0x8D, 0x00),
    reg(0x8E, 0x00),
    reg(0x8F, 0x00),
    reg(0x90, 0x00),
    reg(0x98, 0x00),
    reg(0x99, 0x00),
    reg(0x9A, 0x00),
    reg(0x9B, 0x00),
    reg(0x9C, 0x00),
    reg(0x9D, 0x00),
    reg(0x9E, 0x00),
    reg(0x9F, 0x00),
    reg(0xA0, 0x00),
    reg(0xA1, 0x00),
    reg(0xA2, 0x00),
    reg(0xA3, 0x00),
    reg(0xA4, 0x00),
    reg(0xA5, 0x00),
    reg(0xA6, 0x00),
    reg(0xA7, 0x00),
    reg(0xA8, 0x00),
    reg(0xA9, 0x00),
    reg(0xAA, 0x00),
    reg(0xAB, 0x00),
    reg(0xAC, 0x00),
    reg(0xAD, 0x00),
    reg(0xAE, 0x00),
    reg(0xAF, 0x00),
    reg(0xB0, 0x00),
    reg(0xB1, 0x00),
    reg(0xB2, 0x00),
    reg(0xB3, 0x00),
    reg(0xB4, 0x00),
    reg(0xB5, 0x00),
    reg(0xB6, 0x00),
    reg(0xB7, 0x00),
    reg(0xB8, 0x00),
    reg(0xB9, 0x00),
    reg(0xBA, 0x00),
    reg(0xBB, 0x00),
    reg(0xBC, 0x00),
    reg(0xBD, 0x00),
    reg(0xBE, 0x00),
    reg(0xBF, 0x00),
    reg(0xC0, 0x00),
    reg(0xC1, 0x00),
    reg(0xC2, 0x00),
    reg(0xC3, 0x00),
    reg(0xC4, 0x00),
    reg(0xC5, 0x00),
    reg(0xC6, 0x00),
    reg(0xC7, 0x00),
    reg(0xC8, 0x00),
    reg(0xC9, 0x00),
    reg(0xCA, 0x00),
    reg(0xCB, 0x00),
    reg(0xCC, 0x00),
    reg(0xCD, 0x00),
    reg(0xCE, 0x00),
    reg(0xCF, 0x00),
    reg(0xD0, 0x00),
    reg(0xD1, 0x00),
    reg(0xD2, 0x00),
    reg(0xD3, 0x00),
    reg(0xD4, 0x00),
    reg(0xD5, 0x00),
    reg(0xD6, 0x00),
    reg(0xD7, 0x00),
    reg(0xD8, 0x00),
    reg(0xD9, 0x00),
    reg(0xE6, 0x06),
];

const fn reg(register: u8, value: u8) -> ClockChipRegister {
    ClockChipRegister { register, value }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockChipProgramOptions {
    pub verify: bool,
    pub reset: bool,
    pub ranges: Vec<ClockChipRange>,
}

impl Default for ClockChipProgramOptions {
    fn default() -> Self {
        Self {
            verify: false,
            reset: true,
            ranges: Vec::new(),
        }
    }
}

impl ClockChipProgramOptions {
    pub fn includes(&self, register: u8) -> bool {
        self.ranges.is_empty() || self.ranges.iter().any(|range| range.contains(register))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockChipRange {
    pub start: u8,
    pub end: u8,
}

impl ClockChipRange {
    pub fn new(start: u8, end: u8) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    pub fn contains(self, register: u8) -> bool {
        register >= self.start && register <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockChipReport {
    pub written: usize,
    pub verified: usize,
    pub reset_pulses: usize,
}

pub trait ClockChipBus {
    type Error: fmt::Display;

    fn write_register(&mut self, register: u8, value: u8) -> Result<(), Self::Error>;
    fn read_register(&mut self, register: u8) -> Result<u8, Self::Error>;
    fn reset_delay(&mut self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockChipError<E> {
    Bus {
        register: u8,
        operation: ClockChipOperation,
        source: E,
    },
    VerifyMismatch {
        register: u8,
        expected: u8,
        actual: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockChipOperation {
    Read,
    Write,
}

impl<E: fmt::Display> fmt::Display for ClockChipError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bus {
                register,
                operation,
                source,
            } => write!(
                f,
                "clock-chip {operation} failed at register 0x{register:02X}: {source}"
            ),
            Self::VerifyMismatch {
                register,
                expected,
                actual,
            } => write!(
                f,
                "clock-chip verify mismatch at register 0x{register:02X}: expected 0x{expected:02X}, got 0x{actual:02X}"
            ),
        }
    }
}

impl fmt::Display for ClockChipOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => f.write_str("read"),
            Self::Write => f.write_str("write"),
        }
    }
}

pub fn program_clock_chip<B: ClockChipBus>(
    bus: &mut B,
    options: &ClockChipProgramOptions,
) -> Result<ClockChipReport, ClockChipError<B::Error>> {
    let mut report = ClockChipReport {
        written: 0,
        verified: 0,
        reset_pulses: 0,
    };

    for entry in CLOCKCHIP_REGISTERS {
        if !options.includes(entry.register) {
            continue;
        }
        bus.write_register(entry.register, entry.value)
            .map_err(|source| ClockChipError::Bus {
                register: entry.register,
                operation: ClockChipOperation::Write,
                source,
            })?;
        report.written += 1;

        if options.verify {
            verify_register(bus, *entry)?;
            report.verified += 1;
        }
    }

    if options.reset {
        for value in CLOCKCHIP_RESET_SEQUENCE {
            bus.write_register(CLOCKCHIP_RESET_REGISTER, *value)
                .map_err(|source| ClockChipError::Bus {
                    register: CLOCKCHIP_RESET_REGISTER,
                    operation: ClockChipOperation::Write,
                    source,
                })?;
            report.reset_pulses += 1;
            bus.reset_delay();
        }
    }

    Ok(report)
}

pub fn verify_clock_chip<B: ClockChipBus>(
    bus: &mut B,
    ranges: &[ClockChipRange],
) -> Result<ClockChipReport, ClockChipError<B::Error>> {
    let options = ClockChipProgramOptions {
        verify: true,
        reset: false,
        ranges: ranges.to_vec(),
    };
    let mut report = ClockChipReport {
        written: 0,
        verified: 0,
        reset_pulses: 0,
    };

    for entry in CLOCKCHIP_REGISTERS {
        if !options.includes(entry.register) {
            continue;
        }
        verify_register(bus, *entry)?;
        report.verified += 1;
    }

    Ok(report)
}

fn verify_register<B: ClockChipBus>(
    bus: &mut B,
    entry: ClockChipRegister,
) -> Result<(), ClockChipError<B::Error>> {
    let actual = bus
        .read_register(entry.register)
        .map_err(|source| ClockChipError::Bus {
            register: entry.register,
            operation: ClockChipOperation::Read,
            source,
        })?;
    if actual != entry.value {
        return Err(ClockChipError::VerifyMismatch {
            register: entry.register,
            expected: entry.value,
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::convert::Infallible;

    #[derive(Default)]
    struct FakeBus {
        registers: BTreeMap<u8, u8>,
        writes: Vec<(u8, u8)>,
        delays: usize,
    }

    impl ClockChipBus for FakeBus {
        type Error = Infallible;

        fn write_register(&mut self, register: u8, value: u8) -> Result<(), Self::Error> {
            self.registers.insert(register, value);
            self.writes.push((register, value));
            Ok(())
        }

        fn read_register(&mut self, register: u8) -> Result<u8, Self::Error> {
            Ok(*self.registers.get(&register).unwrap_or(&0))
        }

        fn reset_delay(&mut self) {
            self.delays += 1;
        }
    }

    #[test]
    fn programs_and_verifies_selected_range() {
        let mut bus = FakeBus::default();
        let options = ClockChipProgramOptions {
            verify: true,
            reset: false,
            ranges: vec![ClockChipRange::new(0x1C, 0x1E)],
        };

        let report = program_clock_chip(&mut bus, &options).unwrap();

        assert_eq!(report.written, 3);
        assert_eq!(report.verified, 3);
        assert_eq!(report.reset_pulses, 0);
        assert_eq!(bus.writes, vec![(0x1C, 0x0B), (0x1D, 0x08), (0x1E, 0xB0)]);
    }

    #[test]
    fn reset_writes_are_explicit() {
        let mut bus = FakeBus::default();
        let options = ClockChipProgramOptions {
            verify: false,
            reset: true,
            ranges: vec![ClockChipRange::new(0xE6, 0xE6)],
        };

        let report = program_clock_chip(&mut bus, &options).unwrap();

        assert_eq!(report.written, 1);
        assert_eq!(report.reset_pulses, 2);
        assert_eq!(bus.delays, 2);
        assert_eq!(
            &bus.writes[1..],
            &[
                (CLOCKCHIP_RESET_REGISTER, 0x02),
                (CLOCKCHIP_RESET_REGISTER, 0x00)
            ]
        );
    }

    #[test]
    fn verify_detects_mismatch() {
        let mut bus = FakeBus::default();
        bus.registers.insert(0xE6, 0x00);

        let err = verify_clock_chip(&mut bus, &[ClockChipRange::new(0xE6, 0xE6)]).unwrap_err();

        assert!(matches!(
            err,
            ClockChipError::VerifyMismatch {
                register: 0xE6,
                expected: CLOCKCHIP_SANITY_VALUE,
                actual: 0x00,
            }
        ));
    }
}
