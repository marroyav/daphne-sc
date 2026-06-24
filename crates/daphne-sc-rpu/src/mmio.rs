use crate::AfeHardware;
use daphne_sc_core::afe::{AFE_COUNT, CHANNELS_PER_AFE, CHANNEL_COUNT};
use daphne_sc_core::afe_functions::{
    extract_afe_function_bits, replace_afe_function_bits, validate_afe_function_value,
    AfeFunctionError,
};
use daphne_sc_core::afe_hw::{
    afe_control_offset, afe_dac_offset_offset, afe_dac_trim_offset, afe_register_address_word,
    afe_register_word, dac_channel_is_high_half, dac_companion_channel, dac_gain_bias_offset,
    dac_gain_bias_word, dac_pair_word, dac_trim_offset_half, frontend_bitslip_offset,
    frontend_delay_offset, spy_buffer_frame_clock_offset, AFE_BIAS_DAC_ROUTES, AFE_GAIN_DAC_ROUTES,
    AFE_GLOBAL_BUSY_BITS, AFE_GLOBAL_CONTROL_OFFSET, AFE_GLOBAL_POWERSTATE_BIT,
    AFE_GLOBAL_RESET_BIT, AFE_SPI_IDLE_WORD, AFE_SPI_TRIGGER_WORD, BIAS_ENABLE_OFFSET,
    DAC_GAIN_BIAS_CONTROL_OFFSET, FRONTEND_BITSLIP_TAPS, FRONTEND_CONTROL_OFFSET,
    FRONTEND_DELAYCTRL_READY_BIT, FRONTEND_DELAYCTRL_RESET_BIT, FRONTEND_DELAY_EN_VTC_BIT,
    FRONTEND_DELAY_TAPS, FRONTEND_EXPECTED_FCLK_WORD, FRONTEND_SERDES_RESET_BIT,
    FRONTEND_STATUS_OFFSET, FRONTEND_TRIGGER_OFFSET, FRONTEND_TRIGGER_WORD, FRONTEND_VERIFY_READS,
    VBIAS_DAC_ROUTE,
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
    InvalidFunction,
    InvalidFunctionField,
    AlignmentFailed,
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

    fn wait_frontend_delayctrl_ready(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        for _ in 0..self.config.spin_limit {
            let value = self.read(FRONTEND_STATUS_OFFSET)?;
            if bit(value, FRONTEND_DELAYCTRL_READY_BIT) != 0 {
                return Ok(());
            }
        }
        Err(MmioAfeError::Timeout)
    }

    fn pulse_frontend_control_bit(&mut self, bit_index: u8) -> Result<(), MmioAfeError<I::Error>> {
        self.set_bit(FRONTEND_CONTROL_OFFSET, bit_index, true)?;
        self.set_bit(FRONTEND_CONTROL_OFFSET, bit_index, false)
    }

    fn set_frontend_delay_vtc(&mut self, enabled: bool) -> Result<(), MmioAfeError<I::Error>> {
        self.set_bit(FRONTEND_CONTROL_OFFSET, FRONTEND_DELAY_EN_VTC_BIT, enabled)
    }

    fn reset_frontend_delay_values(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        for afe_board in 0..AFE_COUNT {
            self.set_frontend_bitslip(afe_board, 0)?;
            self.set_frontend_delay(afe_board, 0)?;
        }
        Ok(())
    }

    fn set_frontend_delay(
        &mut self,
        afe_board: u8,
        value: u32,
    ) -> Result<(), MmioAfeError<I::Error>> {
        let offset = frontend_delay_offset(afe_board).ok_or(MmioAfeError::InvalidAfe(afe_board))?;
        if value >= FRONTEND_DELAY_TAPS {
            return Err(MmioAfeError::InvalidValue(value));
        }
        self.write(offset, value)
    }

    fn set_frontend_bitslip(
        &mut self,
        afe_board: u8,
        value: u32,
    ) -> Result<(), MmioAfeError<I::Error>> {
        let offset =
            frontend_bitslip_offset(afe_board).ok_or(MmioAfeError::InvalidAfe(afe_board))?;
        if value >= FRONTEND_BITSLIP_TAPS {
            return Err(MmioAfeError::InvalidValue(value));
        }
        self.write(offset, value)
    }

    fn read_frontend_bitslip(&mut self, afe_board: u8) -> Result<u32, MmioAfeError<I::Error>> {
        let offset =
            frontend_bitslip_offset(afe_board).ok_or(MmioAfeError::InvalidAfe(afe_board))?;
        Ok(self.read(offset)? & 0x0F)
    }

    fn trigger_frontend_snapshot(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        self.write(FRONTEND_TRIGGER_OFFSET, FRONTEND_TRIGGER_WORD)
    }

    fn read_frame_clock(&mut self, afe_board: u8) -> Result<u32, MmioAfeError<I::Error>> {
        let offset = spy_buffer_frame_clock_offset(afe_board, 0)
            .ok_or(MmioAfeError::InvalidAfe(afe_board))?;
        self.read(offset)
    }

    fn scan_frontend_word_after_write(
        &mut self,
        afe_board: u8,
    ) -> Result<u32, MmioAfeError<I::Error>> {
        self.trigger_frontend_snapshot()?;
        self.read_frame_clock(afe_board)
    }

    fn set_best_frontend_delay(&mut self, afe_board: u8) -> Result<(), MmioAfeError<I::Error>> {
        let mut best_start = 0_u32;
        let mut best_len = 0_u32;
        let mut current_start = 0_u32;
        let mut current_len = 0_u32;
        let mut previous_word = 0_u32;
        let mut have_previous = false;

        for tap in 0..FRONTEND_DELAY_TAPS {
            self.set_frontend_delay(afe_board, tap)?;
            let word = self.scan_frontend_word_after_write(afe_board)?;

            if have_previous && word == previous_word {
                current_len += 1;
            } else {
                if current_len > best_len {
                    best_start = current_start;
                    best_len = current_len;
                }
                current_start = tap;
                current_len = 1;
                previous_word = word;
                have_previous = true;
            }
        }

        if current_len > best_len {
            best_start = current_start;
            best_len = current_len;
        }
        if best_len == 0 {
            return Err(MmioAfeError::AlignmentFailed);
        }

        self.set_frontend_delay(afe_board, best_start + ((best_len - 1) / 2))
    }

    fn set_best_frontend_bitslip(&mut self, afe_board: u8) -> Result<(), MmioAfeError<I::Error>> {
        let initial_bitslip = self.read_frontend_bitslip(afe_board)?;
        let mut matched_bitslip = None;

        for tap in 0..FRONTEND_BITSLIP_TAPS {
            self.set_frontend_bitslip(afe_board, tap)?;
            let word = self.scan_frontend_word_after_write(afe_board)?;
            if matched_bitslip.is_none() && word == FRONTEND_EXPECTED_FCLK_WORD {
                matched_bitslip = Some(tap);
            }
        }

        let final_bitslip = if let Some(tap) = matched_bitslip {
            tap
        } else {
            self.set_frontend_bitslip(afe_board, initial_bitslip)?;
            return Err(MmioAfeError::AlignmentFailed);
        };

        self.set_frontend_bitslip(afe_board, final_bitslip)?;
        let aligned_word = self.scan_frontend_word_after_write(afe_board)?;
        if aligned_word != FRONTEND_EXPECTED_FCLK_WORD {
            return Err(MmioAfeError::AlignmentFailed);
        }

        for _ in 0..FRONTEND_VERIFY_READS {
            let verify_word = self.scan_frontend_word_after_write(afe_board)?;
            if verify_word != FRONTEND_EXPECTED_FCLK_WORD {
                return Err(MmioAfeError::AlignmentFailed);
            }
        }

        Ok(())
    }

    fn align_frontend(&mut self) -> Result<(), MmioAfeError<I::Error>> {
        self.reset_frontend_delay_values()?;
        self.pulse_frontend_control_bit(FRONTEND_DELAYCTRL_RESET_BIT)?;
        self.pulse_frontend_control_bit(FRONTEND_SERDES_RESET_BIT)?;
        self.set_frontend_delay_vtc(false)?;
        self.wait_frontend_delayctrl_ready()?;

        let mut result = Ok(());
        for afe_board in 0..AFE_COUNT {
            if let Err(err) = self.set_best_frontend_delay(afe_board) {
                result = Err(err);
                break;
            }
            if let Err(err) = self.set_best_frontend_bitslip(afe_board) {
                result = Err(err);
                break;
            }
        }

        let vtc_result = self.set_frontend_delay_vtc(true);
        result?;
        vtc_result
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

    fn apply_staged_channel(
        &mut self,
        config: RpuWireChannelConfig,
    ) -> Result<(), MmioAfeError<I::Error>> {
        let trim = require_12bit(u32::from(config.trim))?;
        let offset = require_12bit(u32::from(config.offset))?;

        self.write_trim_or_offset_channel(
            RpuWireOp::SetTrim,
            config.afe_pl,
            config.afe_channel,
            trim,
            false,
        )?;
        self.write_trim_or_offset_channel(
            RpuWireOp::SetOffset,
            config.afe_pl,
            config.afe_channel,
            offset,
            false,
        )
    }

    fn apply_staged_afe(&mut self, config: RpuWireAfeConfig) -> Result<(), MmioAfeError<I::Error>> {
        let attenuation = require_12bit(u32::from(config.attenuation))?;
        let attenuation_route = AFE_GAIN_DAC_ROUTES[usize::from(config.afe_pl)];
        self.write_dac_route(attenuation_route, attenuation)?;
        self.attenuation[usize::from(config.afe_pl)] = attenuation;

        if config.bias != 0 {
            let bias = require_12bit(u32::from(config.bias))?;
            let bias_route = AFE_BIAS_DAC_ROUTES[usize::from(config.afe_pl)];
            self.write_dac_route(bias_route, bias)?;
            self.bias[usize::from(config.afe_pl)] = bias;
        }

        self.write_afe_function(config.afe_pl, "SERIALIZED_DATA_RATE", 1)?;
        self.write_afe_function(
            config.afe_pl,
            "ADC_RESOLUTION_RESET",
            u16::from(config.adc.resolution),
        )?;
        self.write_afe_function(
            config.afe_pl,
            "ADC_OUTPUT_FORMAT",
            u16::from(config.adc.output_format),
        )?;
        self.write_afe_function(
            config.afe_pl,
            "LSB_MSB_FIRST",
            u16::from(config.adc.msb_first),
        )?;

        self.write_afe_function(
            config.afe_pl,
            "LPF_PROGRAMMABILITY",
            u16::from(config.pga.lpf_cut_frequency),
        )?;
        self.write_afe_function(
            config.afe_pl,
            "PGA_INTEGRATOR_DISABLE",
            u16::from(config.pga.integrator_disable),
        )?;
        self.write_afe_function(
            config.afe_pl,
            "PGA_GAIN_CONTROL",
            u16::from(config.pga.gain),
        )?;
        self.write_afe_function(config.afe_pl, "PGA_CLAMP_LEVEL", 2)?;
        self.write_afe_function(config.afe_pl, "ACTIVE_TERMINATION_ENABLE", 0)?;

        self.write_afe_function(
            config.afe_pl,
            "LNA_INPUT_CLAMP_SETTING",
            u16::from(config.lna.clamp),
        )?;
        self.write_afe_function(config.afe_pl, "LNA_GAIN", u16::from(config.lna.gain))?;
        self.write_afe_function(
            config.afe_pl,
            "LNA_INTEGRATOR_DISABLE",
            u16::from(config.lna.integrator_disable),
        )?;

        Ok(())
    }

    fn write_afe_function(
        &mut self,
        afe_pl: u8,
        name: &str,
        value: u16,
    ) -> Result<u32, MmioAfeError<I::Error>> {
        validate_afe(afe_pl)?;
        let spec =
            validate_afe_function_value(name, value).map_err(|err| function_error(err, value))?;
        spec.field
            .mask()
            .map_err(|err| function_error(err, value))?;

        let current = self.read_register(afe_pl, spec.field.register)? as u16;
        let updated = replace_afe_function_bits(current, spec.field, value)
            .map_err(|err| function_error(err, value))?;

        self.write_register(afe_pl, spec.field.register, u32::from(updated))?;

        let readback = self.read_register(afe_pl, spec.field.register)? as u16;
        extract_afe_function_bits(readback, spec.field)
            .map(u32::from)
            .map_err(|err| function_error(err, value))
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
        self.align_frontend()
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
        self.staged_config.validate_counts()?;
        let staged = self.staged_config;

        self.do_reset()?;
        self.set_power_state(true)?;

        for config in staged.channels.iter().flatten().copied() {
            self.apply_staged_channel(config)?;
        }

        self.write_vbias_control(u32::from(staged.counts.bias_control), true)?;

        for config in staged.afes.iter().flatten().copied() {
            self.apply_staged_afe(config)?;
        }

        self.set_power_state(true)?;
        self.staged_config = StagedConfig::default();
        Ok(())
    }

    fn write_function(&mut self, afe_pl: u8, name: &str, value: u16) -> Result<u32, Self::Error> {
        self.write_afe_function(afe_pl, name, value)
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
    counts: RpuWireConfigCounts,
    afes: [Option<RpuWireAfeConfig>; AFE_COUNT as usize],
    channels: [Option<RpuWireChannelConfig>; CHANNEL_COUNT as usize],
}

impl StagedConfig {
    const fn new(counts: RpuWireConfigCounts) -> Self {
        Self {
            active: true,
            counts,
            afes: [None; AFE_COUNT as usize],
            channels: [None; CHANNEL_COUNT as usize],
        }
    }

    fn validate_counts<E>(&self) -> Result<(), MmioAfeError<E>> {
        if self.received_afes() == self.counts.afe_count
            && self.received_channels() == self.counts.channel_count
        {
            Ok(())
        } else {
            Err(MmioAfeError::Unsupported)
        }
    }

    fn received_afes(&self) -> u16 {
        self.afes.iter().flatten().count() as u16
    }

    fn received_channels(&self) -> u16 {
        self.channels.iter().flatten().count() as u16
    }
}

impl Default for StagedConfig {
    fn default() -> Self {
        Self {
            active: false,
            counts: RpuWireConfigCounts {
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

fn function_error<E>(err: AfeFunctionError, value: u16) -> MmioAfeError<E> {
    match err {
        AfeFunctionError::UnknownName => MmioAfeError::InvalidFunction,
        AfeFunctionError::InvalidValue => MmioAfeError::InvalidValue(u32::from(value)),
        AfeFunctionError::InvalidBitField => MmioAfeError::InvalidFunctionField,
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
    use daphne_sc_core::afe::{AdcConfig, LnaConfig, PgaConfig};
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

        fn with_frontend_ready(mut self) -> Self {
            self.registers.insert(FRONTEND_STATUS_OFFSET, 1);
            self
        }

        fn with_alignment_success_reads(mut self, bitslip: u32) -> Self {
            for afe_board in 0..AFE_COUNT {
                let frame_clock = spy_buffer_frame_clock_offset(afe_board, 0).unwrap();
                for _ in 0..FRONTEND_DELAY_TAPS {
                    self.reads
                        .push((frame_clock, 0x1111_0000 + u32::from(afe_board)));
                }
                for tap in 0..FRONTEND_BITSLIP_TAPS {
                    let word = if tap == bitslip {
                        FRONTEND_EXPECTED_FCLK_WORD
                    } else {
                        0
                    };
                    self.reads.push((frame_clock, word));
                }
                for _ in 0..=FRONTEND_VERIFY_READS {
                    self.reads.push((frame_clock, FRONTEND_EXPECTED_FCLK_WORD));
                }
            }
            self
        }

        fn with_first_afe_alignment_failure_reads(mut self) -> Self {
            let frame_clock = spy_buffer_frame_clock_offset(0, 0).unwrap();
            for _ in 0..FRONTEND_DELAY_TAPS {
                self.reads.push((frame_clock, 0x1111_0000));
            }
            for _ in 0..FRONTEND_BITSLIP_TAPS {
                self.reads.push((frame_clock, 0));
            }
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
    fn align_scans_frontend_and_sets_delay_bitslip() {
        let fake = FakeIo::default()
            .with_frontend_ready()
            .with_alignment_success_reads(3);
        let mut backend = AfeMmioBackend::new(fake);

        backend.align().unwrap();

        let io = backend.io_mut();
        for afe_board in 0..AFE_COUNT {
            assert_eq!(
                io.registers[&frontend_delay_offset(afe_board).unwrap()],
                (FRONTEND_DELAY_TAPS - 1) / 2
            );
            assert_eq!(
                io.registers[&frontend_bitslip_offset(afe_board).unwrap()],
                3
            );
        }
        assert_eq!(
            io.registers[&FRONTEND_CONTROL_OFFSET],
            1 << FRONTEND_DELAY_EN_VTC_BIT
        );
        assert!(io
            .writes
            .contains(&(FRONTEND_TRIGGER_OFFSET, FRONTEND_TRIGGER_WORD)));
    }

    #[test]
    fn align_reenables_vtc_after_pattern_failure() {
        let fake = FakeIo::default()
            .with_frontend_ready()
            .with_first_afe_alignment_failure_reads();
        let mut backend = AfeMmioBackend::new(fake);

        let err = backend.align().unwrap_err();

        assert!(matches!(err, MmioAfeError::AlignmentFailed));
        assert_eq!(
            backend.io_mut().registers[&FRONTEND_CONTROL_OFFSET],
            1 << FRONTEND_DELAY_EN_VTC_BIT
        );
    }

    #[test]
    fn config_apply_rejects_incomplete_staged_counts_before_mmio() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());
        backend
            .begin_configure_frontend(RpuWireConfigCounts {
                afe_count: 1,
                channel_count: 0,
                bias_control: 0,
            })
            .unwrap();

        let err = backend.apply_configure_frontend().unwrap_err();

        assert!(matches!(err, MmioAfeError::Unsupported));
        assert!(backend.io_mut().writes.is_empty());
    }

    #[test]
    fn full_config_apply_programs_staged_frontend_controls() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());
        backend
            .begin_configure_frontend(RpuWireConfigCounts {
                afe_count: 1,
                channel_count: 1,
                bias_control: 0x0666,
            })
            .unwrap();
        backend
            .configure_channel(RpuWireChannelConfig {
                channel: 10,
                afe_board: 1,
                afe_pl: 4,
                afe_channel: 2,
                trim: 0x00AA,
                offset: 0x00BB,
                gain: 0,
            })
            .unwrap();
        backend
            .configure_afe(RpuWireAfeConfig {
                afe_board: 1,
                afe_pl: 4,
                attenuation: 0x0555,
                bias: 0x0444,
                adc: AdcConfig {
                    resolution: true,
                    output_format: false,
                    msb_first: true,
                },
                pga: PgaConfig {
                    lpf_cut_frequency: 3,
                    integrator_disable: true,
                    gain: true,
                },
                lna: LnaConfig {
                    clamp: 2,
                    gain: 3,
                    integrator_disable: true,
                },
            })
            .unwrap();

        backend.apply_configure_frontend().unwrap();

        let writes = backend.io_mut().writes.as_slice();
        assert!(writes.contains(&(0x38, 0x0000_80AA)));
        assert!(writes.contains(&(0x3C, 0x0000_80BB)));
        assert!(writes.contains(&(
            dac_gain_bias_offset(VBIAS_DAC_ROUTE.chip),
            dac_gain_bias_word(VBIAS_DAC_ROUTE, 0x0666)
        )));
        assert!(writes.contains(&(BIAS_ENABLE_OFFSET, 1)));
        assert!(writes.contains(&(
            dac_gain_bias_offset(AFE_GAIN_DAC_ROUTES[4].chip),
            dac_gain_bias_word(AFE_GAIN_DAC_ROUTES[4], 0x0555)
        )));
        assert!(writes.contains(&(
            dac_gain_bias_offset(AFE_BIAS_DAC_ROUTES[4].chip),
            dac_gain_bias_word(AFE_BIAS_DAC_ROUTES[4], 0x0444)
        )));
        assert!(writes.contains(&(0x34, afe_register_word(3, 0x2000))));
        assert!(writes.contains(&(0x34, afe_register_word(51, 0x0006))));
        assert!(writes.contains(&(0x34, afe_register_word(51, 0x2000))));
        assert_eq!(
            writes.last(),
            Some(&(AFE_GLOBAL_CONTROL_OFFSET, 0x0000_0002))
        );
    }

    #[test]
    fn write_function_uses_legacy_read_modify_write_sequence() {
        let fake = FakeIo::default()
            .with_read(0x04, 0xFFFF)
            .with_read(0x04, 0xFFF9)
            .with_read(0x04, 0xFFF9);
        let mut backend = AfeMmioBackend::new(fake);

        let readback = backend.write_function(0, "LPF_PROGRAMMABILITY", 4).unwrap();

        assert_eq!(readback, 4);
        assert_eq!(
            backend.io_mut().writes.as_slice(),
            &[
                (0x04, AFE_SPI_TRIGGER_WORD),
                (0x04, 0x0033_0000),
                (0x04, AFE_SPI_IDLE_WORD),
                (0x04, 0x0033_FFF9),
                (0x04, AFE_SPI_TRIGGER_WORD),
                (0x04, 0x0033_0000),
                (0x04, AFE_SPI_IDLE_WORD),
                (0x04, AFE_SPI_TRIGGER_WORD),
                (0x04, 0x0033_0000),
                (0x04, AFE_SPI_IDLE_WORD),
            ]
        );
    }

    #[test]
    fn write_function_rejects_invalid_option_before_mmio() {
        let mut backend = AfeMmioBackend::new(FakeIo::default());

        let err = backend
            .write_function(0, "LPF_PROGRAMMABILITY", 1)
            .unwrap_err();

        assert!(matches!(err, MmioAfeError::InvalidValue(1)));
        assert!(backend.io_mut().writes.is_empty());
    }

    #[test]
    fn write_function_rejects_malformed_legacy_bitfield() {
        let mut backend = AfeMmioBackend::new(FakeIo::default().with_read(0x04, 0));

        let err = backend
            .write_function(0, "LVDS_OUTPUT_RATE_2X", 1)
            .unwrap_err();

        assert!(matches!(err, MmioAfeError::InvalidFunctionField));
        assert!(backend.io_mut().writes.is_empty());
    }
}
