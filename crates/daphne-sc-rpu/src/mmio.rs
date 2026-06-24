use crate::AfeHardware;
use daphne_sc_core::afe::{AFE_COUNT, CHANNELS_PER_AFE, CHANNEL_COUNT};
use daphne_sc_core::afe_hw::{
    afe_control_offset, afe_dac_offset_offset, afe_dac_trim_offset, afe_register_address_word,
    afe_register_word, dac_channel_is_high_half, dac_companion_channel, dac_gain_bias_offset,
    dac_gain_bias_word, dac_pair_word, dac_trim_offset_half, AFE_BIAS_DAC_ROUTES,
    AFE_GAIN_DAC_ROUTES, AFE_GLOBAL_BUSY_BITS, AFE_GLOBAL_CONTROL_OFFSET,
    AFE_GLOBAL_POWERSTATE_BIT, AFE_GLOBAL_RESET_BIT, AFE_SPI_IDLE_WORD, AFE_SPI_TRIGGER_WORD,
    BIAS_ENABLE_OFFSET, DAC_GAIN_BIAS_CONTROL_OFFSET, VBIAS_DAC_ROUTE,
};
use daphne_sc_core::{
    RpuWireAfeConfig, RpuWireChannelConfig, RpuWireCommand, RpuWireConfigCounts, RpuWireOp,
    RpuWireTarget,
};

const DEFAULT_SPIN_LIMIT: u32 = 100_000;
const DAC_GO_BIT: u8 = 1;
const DAC_BUSY_BIT: u8 = 0;
const DAC_12BIT_MAX: u32 = 0x0FFF;

pub trait RegisterIo {
    type Error;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error>;
    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioAfeConfig {
    pub spin_limit: u32,
}

impl Default for MmioAfeConfig {
    fn default() -> Self {
        Self {
            spin_limit: DEFAULT_SPIN_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmioAfeError<E> {
    Io(E),
    InvalidAfe(u8),
    InvalidChannel(u8),
    InvalidValue(u32),
    Timeout,
    Unsupported,
}

pub struct AfeMmioBackend<I> {
    io: I,
    config: MmioAfeConfig,
    attenuation: [u16; AFE_COUNT as usize],
    bias: [u16; AFE_COUNT as usize],
    trim_halves: [[u16; CHANNELS_PER_AFE as usize]; AFE_COUNT as usize],
    offset_halves: [[u16; CHANNELS_PER_AFE as usize]; AFE_COUNT as usize],
    vbias_control: u16,
    vbias_enabled: bool,
    staged_config: StagedConfig,
}

impl<I> AfeMmioBackend<I> {
    pub fn new(io: I) -> Self {
        Self::with_config(io, MmioAfeConfig::default())
    }

    pub fn with_config(io: I, config: MmioAfeConfig) -> Self {
        Self {
            io,
            config,
            attenuation: [0; AFE_COUNT as usize],
            bias: [0; AFE_COUNT as usize],
            trim_halves: [[0; CHANNELS_PER_AFE as usize]; AFE_COUNT as usize],
            offset_halves: [[0; CHANNELS_PER_AFE as usize]; AFE_COUNT as usize],
            vbias_control: 0,
            vbias_enabled: false,
            staged_config: StagedConfig::default(),
        }
    }

    pub fn io_mut(&mut self) -> &mut I {
        &mut self.io
    }
}

impl<I: RegisterIo> AfeMmioBackend<I> {
    fn read(&mut self, offset: u32) -> Result<u32, MmioAfeError<I::Error>> {
        self.io.read32(offset).map_err(MmioAfeError::Io)
    }

    fn write(&mut self, offset: u32, value: u32) -> Result<(), MmioAfeError<I::Error>> {
        self.io.write32(offset, value).map_err(MmioAfeError::Io)
    }

    fn wait_afe_ready(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        for _ in 0..self.config.spin_limit {
            let value = self.read(AFE_GLOBAL_CONTROL_OFFSET)?;
            if bits(
                value,
                *AFE_GLOBAL_BUSY_BITS.start(),
                *AFE_GLOBAL_BUSY_BITS.end(),
            ) == 0
            {
                return Ok(());
            }
        }
        Err(MmioAfeError::Timeout)
    }

    fn wait_dac_ready(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        for _ in 0..self.config.spin_limit {
            let value = self.read(DAC_GAIN_BIAS_CONTROL_OFFSET)?;
            if bit(value, DAC_BUSY_BIT) == 0 {
                return Ok(());
            }
        }
        Err(MmioAfeError::Timeout)
    }

    fn trigger_dac(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        self.wait_dac_ready()?;
        self.set_bit(DAC_GAIN_BIAS_CONTROL_OFFSET, DAC_GO_BIT, true)?;
        self.wait_dac_ready()?;
        self.set_bit(DAC_GAIN_BIAS_CONTROL_OFFSET, DAC_GO_BIT, false)?;
        self.wait_dac_ready()
    }

    fn set_bit(
        &mut self,
        offset: u32,
        bit_index: u8,
        asserted: bool,
    ) -> Result<(), MmioAfeError<I::Error>> {
        let mut value = self.read(offset)?;
        let mask = 1_u32 << bit_index;
        if asserted {
            value |= mask;
        } else {
            value &= !mask;
        }
        self.write(offset, value)
    }

    fn write_dac_route(
        &mut self,
        route: daphne_sc_core::afe_hw::DacRoute,
        value: u16,
    ) -> Result<(), MmioAfeError<I::Error>> {
        self.wait_dac_ready()?;
        self.write(
            dac_gain_bias_offset(route.chip),
            dac_gain_bias_word(route, value),
        )?;
        self.trigger_dac()
    }

    fn write_trim_or_offset_channel(
        &mut self,
        op: RpuWireOp,
        afe_pl: u8,
        afe_channel: u8,
        value: u16,
        gain: bool,
    ) -> Result<(), MmioAfeError<I::Error>> {
        let offset = match op {
            RpuWireOp::SetTrim => afe_dac_trim_offset(afe_pl),
            RpuWireOp::SetOffset => afe_dac_offset_offset(afe_pl),
            _ => return Err(MmioAfeError::Unsupported),
        }
        .ok_or(MmioAfeError::InvalidAfe(afe_pl))?;

        let companion =
            dac_companion_channel(afe_channel).ok_or(MmioAfeError::InvalidChannel(afe_channel))?;
        let afe_index = usize::from(afe_pl);
        let channel_index = usize::from(afe_channel);
        let companion_index = usize::from(companion);
        let half = dac_trim_offset_half(afe_channel, value, gain, false);

        let halves = match op {
            RpuWireOp::SetTrim => &mut self.trim_halves[afe_index],
            RpuWireOp::SetOffset => &mut self.offset_halves[afe_index],
            _ => unreachable!(),
        };
        halves[channel_index] = half;
        let companion_half = halves[companion_index];
        let word = if dac_channel_is_high_half(afe_channel) {
            dac_pair_word(companion_half, half)
        } else {
            dac_pair_word(half, companion_half)
        };

        self.wait_afe_ready()?;
        self.write(offset, word)?;
        self.wait_afe_ready()
    }

    fn read_trim_or_offset_channel(
        &self,
        op: RpuWireOp,
        afe_pl: u8,
        afe_channel: u8,
    ) -> Result<u32, MmioAfeError<I::Error>> {
        validate_afe(afe_pl)?;
        if afe_channel >= CHANNELS_PER_AFE {
            return Err(MmioAfeError::InvalidChannel(afe_channel));
        }

        let halves = match op {
            RpuWireOp::ReadTrim => &self.trim_halves[usize::from(afe_pl)],
            RpuWireOp::ReadOffset => &self.offset_halves[usize::from(afe_pl)],
            _ => return Err(MmioAfeError::Unsupported),
        };
        Ok(u32::from(halves[usize::from(afe_channel)] & 0x0FFF))
    }
}

impl<I: RegisterIo> AfeHardware for AfeMmioBackend<I> {
    type Error = MmioAfeError<I::Error>;

    fn read_register(&mut self, afe_pl: u8, register: u16) -> Result<u32, Self::Error> {
        let offset = afe_control_offset(afe_pl).ok_or(MmioAfeError::InvalidAfe(afe_pl))?;
        self.wait_afe_ready()?;
        self.write(offset, AFE_SPI_TRIGGER_WORD)?;
        self.wait_afe_ready()?;
        self.write(offset, afe_register_address_word(register))?;
        self.wait_afe_ready()?;
        let value = self.read(offset)? & 0xFFFF;
        self.write(offset, AFE_SPI_IDLE_WORD)?;
        Ok(value)
    }

    fn write_register(
        &mut self,
        afe_pl: u8,
        register: u16,
        value: u32,
    ) -> Result<u32, Self::Error> {
        let value = require_u16(value)?;
        let offset = afe_control_offset(afe_pl).ok_or(MmioAfeError::InvalidAfe(afe_pl))?;
        self.wait_afe_ready()?;
        self.write(offset, afe_register_word(register, value))?;
        self.wait_afe_ready()?;
        self.write(offset, AFE_SPI_TRIGGER_WORD)?;
        self.wait_afe_ready()?;
        self.write(offset, afe_register_address_word(register))?;
        self.wait_afe_ready()?;
        let readback = self.read(offset)? & 0xFFFF;
        self.write(offset, AFE_SPI_IDLE_WORD)?;
        Ok(readback)
    }

    fn read_scalar(&mut self, op: RpuWireOp, afe_pl: u8) -> Result<u32, Self::Error> {
        validate_afe(afe_pl)?;
        match op {
            RpuWireOp::ReadAttenuation => Ok(u32::from(self.attenuation[usize::from(afe_pl)])),
            RpuWireOp::ReadBias => Ok(u32::from(self.bias[usize::from(afe_pl)])),
            _ => Err(MmioAfeError::Unsupported),
        }
    }

    fn write_scalar(&mut self, op: RpuWireOp, afe_pl: u8, value: u32) -> Result<u32, Self::Error> {
        validate_afe(afe_pl)?;
        let value = require_12bit(value)?;
        match op {
            RpuWireOp::SetAttenuation => {
                let route = AFE_GAIN_DAC_ROUTES[usize::from(afe_pl)];
                self.write_dac_route(route, value)?;
                self.attenuation[usize::from(afe_pl)] = value;
                Ok(u32::from(value))
            }
            RpuWireOp::SetBias => {
                let route = AFE_BIAS_DAC_ROUTES[usize::from(afe_pl)];
                self.write_dac_route(route, value)?;
                self.bias[usize::from(afe_pl)] = value;
                Ok(u32::from(value))
            }
            _ => Err(MmioAfeError::Unsupported),
        }
    }

    fn read_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error> {
        if command.target != RpuWireTarget::Channel {
            return Err(MmioAfeError::Unsupported);
        }
        self.read_trim_or_offset_channel(command.op, command.afe_pl, command.afe_channel)
    }

    fn write_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error> {
        let value = require_12bit(command.value)?;
        let gain = command.flags != 0;
        match command.target {
            RpuWireTarget::Channel => {
                self.write_trim_or_offset_channel(
                    command.op,
                    command.afe_pl,
                    command.afe_channel,
                    value,
                    gain,
                )?;
            }
            RpuWireTarget::Afe => {
                validate_afe(command.afe_pl)?;
                for afe_channel in 0..CHANNELS_PER_AFE {
                    self.write_trim_or_offset_channel(
                        command.op,
                        command.afe_pl,
                        afe_channel,
                        value,
                        gain,
                    )?;
                }
            }
            RpuWireTarget::All => {
                for afe_pl in 0..AFE_COUNT {
                    for afe_channel in 0..CHANNELS_PER_AFE {
                        self.write_trim_or_offset_channel(
                            command.op,
                            afe_pl,
                            afe_channel,
                            value,
                            gain,
                        )?;
                    }
                }
            }
            RpuWireTarget::None => return Err(MmioAfeError::Unsupported),
        }
        Ok(u32::from(value))
    }

    fn read_vbias_control(&mut self) -> Result<u32, Self::Error> {
        Ok(u32::from(self.vbias_control))
    }

    fn write_vbias_control(&mut self, value: u32, enable: bool) -> Result<u32, Self::Error> {
        let value = require_12bit(value)?;
        self.write_dac_route(VBIAS_DAC_ROUTE, value)?;
        self.write(BIAS_ENABLE_OFFSET, u32::from(enable))?;
        self.vbias_control = value;
        self.vbias_enabled = enable;
        Ok(u32::from(value))
    }

    fn set_reset(&mut self, asserted: bool) -> Result<(), Self::Error> {
        self.set_bit(AFE_GLOBAL_CONTROL_OFFSET, AFE_GLOBAL_RESET_BIT, asserted)
    }

    fn do_reset(&mut self) -> Result<(), Self::Error> {
        self.set_reset(true)?;
        self.set_reset(false)
    }

    fn set_power_state(&mut self, enabled: bool) -> Result<(), Self::Error> {
        self.set_bit(
            AFE_GLOBAL_CONTROL_OFFSET,
            AFE_GLOBAL_POWERSTATE_BIT,
            enabled,
        )
    }

    fn align(&mut self) -> Result<(), Self::Error> {
        Err(MmioAfeError::Unsupported)
    }

    fn begin_configure_frontend(&mut self, counts: RpuWireConfigCounts) -> Result<(), Self::Error> {
        self.staged_config = StagedConfig::new(counts);
        Ok(())
    }

    fn configure_afe(&mut self, config: RpuWireAfeConfig) -> Result<(), Self::Error> {
        validate_afe(config.afe_pl)?;
        if !self.staged_config.active {
            return Err(MmioAfeError::Unsupported);
        }
        self.staged_config.afes[usize::from(config.afe_pl)] = Some(config);
        Ok(())
    }

    fn configure_channel(&mut self, config: RpuWireChannelConfig) -> Result<(), Self::Error> {
        validate_afe(config.afe_pl)?;
        if config.afe_channel >= CHANNELS_PER_AFE || config.channel >= CHANNEL_COUNT {
            return Err(MmioAfeError::InvalidChannel(config.channel));
        }
        if !self.staged_config.active {
            return Err(MmioAfeError::Unsupported);
        }
        self.staged_config.channels[usize::from(config.channel)] = Some(config);
        Ok(())
    }

    fn apply_configure_frontend(&mut self) -> Result<(), Self::Error> {
        if !self.staged_config.active {
            return Err(MmioAfeError::Unsupported);
        }
        self.staged_config = StagedConfig::default();
        Err(MmioAfeError::Unsupported)
    }

    fn write_function(
        &mut self,
        _afe_pl: u8,
        _name: &str,
        _value: u16,
    ) -> Result<u32, Self::Error> {
        Err(MmioAfeError::Unsupported)
    }
}

pub struct VolatileRegisterIo {
    base: *mut u32,
}

impl VolatileRegisterIo {
    /// # Safety
    ///
    /// `base` must point at a valid memory-mapped register window for the
    /// lifetime of this object. Callers must ensure exclusive mutable access to
    /// the hardware region represented by the pointer.
    pub const unsafe fn new(base: *mut u32) -> Self {
        Self { base }
    }
}

impl RegisterIo for VolatileRegisterIo {
    type Error = ();

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
        let ptr = unsafe { (self.base.cast::<u8>()).add(offset as usize).cast::<u32>() };
        Ok(unsafe { core::ptr::read_volatile(ptr) })
    }

    fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
        let ptr = unsafe { (self.base.cast::<u8>()).add(offset as usize).cast::<u32>() };
        unsafe { core::ptr::write_volatile(ptr, value) };
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct StagedConfig {
    active: bool,
    _counts: RpuWireConfigCounts,
    afes: [Option<RpuWireAfeConfig>; AFE_COUNT as usize],
    channels: [Option<RpuWireChannelConfig>; CHANNEL_COUNT as usize],
}

impl StagedConfig {
    const fn new(counts: RpuWireConfigCounts) -> Self {
        Self {
            active: true,
            _counts: counts,
            afes: [None; AFE_COUNT as usize],
            channels: [None; CHANNEL_COUNT as usize],
        }
    }
}

impl Default for StagedConfig {
    fn default() -> Self {
        Self {
            active: false,
            _counts: RpuWireConfigCounts {
                afe_count: 0,
                channel_count: 0,
                bias_control: 0,
            },
            afes: [None; AFE_COUNT as usize],
            channels: [None; CHANNEL_COUNT as usize],
        }
    }
}

fn validate_afe<E>(afe_pl: u8) -> Result<(), MmioAfeError<E>> {
    if afe_pl < AFE_COUNT {
        Ok(())
    } else {
        Err(MmioAfeError::InvalidAfe(afe_pl))
    }
}

fn require_u16<E>(value: u32) -> Result<u16, MmioAfeError<E>> {
    u16::try_from(value).map_err(|_| MmioAfeError::InvalidValue(value))
}

fn require_12bit<E>(value: u32) -> Result<u16, MmioAfeError<E>> {
    if value <= DAC_12BIT_MAX {
        Ok(value as u16)
    } else {
        Err(MmioAfeError::InvalidValue(value))
    }
}

fn bit(value: u32, bit_index: u8) -> u32 {
    (value >> bit_index) & 1
}

fn bits(value: u32, start: u8, end: u8) -> u32 {
    let width = end - start + 1;
    (value >> start) & ((1_u32 << width) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::vec::Vec;

    #[derive(Debug, Default)]
    struct FakeIo {
        registers: BTreeMap<u32, u32>,
        reads: Vec<(u32, u32)>,
        read_index: usize,
        writes: Vec<(u32, u32)>,
    }

    impl FakeIo {
        fn with_read(mut self, offset: u32, value: u32) -> Self {
            self.reads.push((offset, value));
            self
        }
    }

    impl RegisterIo for FakeIo {
        type Error = ();

        fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
            if let Some((expected_offset, value)) = self.reads.get(self.read_index) {
                if *expected_offset == offset {
                    self.read_index += 1;
                    return Ok(*value);
                }
            }
            Ok(*self.registers.get(&offset).unwrap_or(&0))
        }

        fn write32(&mut self, offset: u32, value: u32) -> Result<(), Self::Error> {
            self.registers.insert(offset, value);
            self.writes.push((offset, value));
            Ok(())
        }
    }

    #[test]
    fn write_register_follows_cpp_spi_sequence() {
        let fake = FakeIo::default().with_read(0x34, 0x1234);
        let mut backend = AfeMmioBackend::new(fake);

        let readback = backend.write_register(4, 3, 0x1234).unwrap();

        assert_eq!(readback, 0x1234);
        assert_eq!(
            backend.io_mut().writes.as_slice(),
            &[
                (0x34, 0x0003_1234),
                (0x34, AFE_SPI_TRIGGER_WORD),
                (0x34, 0x0003_0000),
                (0x34, AFE_SPI_IDLE_WORD),
            ]
        );
    }

    #[test]
    fn set_attenuation_programs_gain_dac_route() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());

        let readback = backend
            .write_scalar(RpuWireOp::SetAttenuation, 4, 0x0555)
            .unwrap();

        assert_eq!(readback, 0x0555);
        assert_eq!(
            backend.io_mut().writes.as_slice(),
            &[
                (dac_gain_bias_offset(VBIAS_DAC_ROUTE.chip), 0x0000_4555),
                (DAC_GAIN_BIAS_CONTROL_OFFSET, 0x0000_0002),
                (DAC_GAIN_BIAS_CONTROL_OFFSET, 0x0000_0000),
            ]
        );
    }

    #[test]
    fn trim_write_updates_high_half_and_preserves_companion() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());
        let mut command = RpuWireCommand::status(1);
        command.op = RpuWireOp::SetTrim;
        command.target = RpuWireTarget::Channel;
        command.afe_pl = 4;
        command.afe_channel = 6;
        command.value = 0x0555;
        command.flags = 1;

        let readback = backend.write_channel_scalar(&command).unwrap();

        assert_eq!(readback, 0x0555);
        assert_eq!(backend.io_mut().writes.as_slice(), &[(0x38, 0xA555_0000)]);
    }

    #[test]
    fn vbias_write_programs_dac_and_enable_register() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());

        let readback = backend.write_vbias_control(0x0666, true).unwrap();

        assert_eq!(readback, 0x0666);
        assert_eq!(
            backend.io_mut().writes.as_slice(),
            &[
                (dac_gain_bias_offset(VBIAS_DAC_ROUTE.chip), 0x0000_8666),
                (DAC_GAIN_BIAS_CONTROL_OFFSET, 0x0000_0002),
                (DAC_GAIN_BIAS_CONTROL_OFFSET, 0x0000_0000),
                (BIAS_ENABLE_OFFSET, 1),
            ]
        );
    }

    #[test]
    fn busy_wait_times_out() {
        let mut fake = FakeIo::default();
        fake.registers.insert(AFE_GLOBAL_CONTROL_OFFSET, 0x1C);
        let mut backend = AfeMmioBackend::with_config(fake, MmioAfeConfig { spin_limit: 2 });

        let err = backend.write_register(0, 1, 1).unwrap_err();

        assert!(matches!(err, MmioAfeError::Timeout));
    }

    #[test]
    fn full_config_apply_is_explicitly_unsupported_for_now() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());
        backend
            .begin_configure_frontend(RpuWireConfigCounts {
                afe_count: 0,
                channel_count: 0,
                bias_control: 0,
            })
            .unwrap();

        let err = backend.apply_configure_frontend().unwrap_err();

        assert!(matches!(err, MmioAfeError::Unsupported));
    }
}
