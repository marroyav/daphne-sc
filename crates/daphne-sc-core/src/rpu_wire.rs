use crate::afe::{
    AdcConfig, AfeCommand, AfeFrontendConfig, AfeId, ChannelFrontendConfig, ChannelId,
    ChannelTarget, LnaConfig, PgaConfig,
};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::str;

pub const RPU_WIRE_MAGIC: u32 = 0x5250_5344; // "DSPR" little-endian marker
pub const RPU_WIRE_ABI_VERSION: u16 = 2;
pub const RPU_WIRE_COMMAND_LEN: usize = 64;
pub const RPU_WIRE_REPLY_LEN: usize = 64;
pub const RPU_WIRE_PAYLOAD_LEN: usize = 32;
pub const RPU_WIRE_FUNCTION_NAME_MAX: usize = RPU_WIRE_PAYLOAD_LEN - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum RpuWireOp {
    Status = 0,
    ReadRegister = 1,
    WriteRegister = 2,
    ReadAttenuation = 3,
    SetAttenuation = 4,
    ReadBias = 5,
    SetBias = 6,
    ReadTrim = 7,
    SetTrim = 8,
    ReadOffset = 9,
    SetOffset = 10,
    ReadVbiasControl = 11,
    SetVbiasControl = 12,
    SetReset = 13,
    DoReset = 14,
    SetPowerState = 15,
    Align = 16,
    BeginConfigureFrontend = 17,
    ConfigureAfe = 18,
    ConfigureChannel = 19,
    ApplyConfigureFrontend = 20,
    WriteFunction = 21,
    ReadAlignment = 22,
}

impl TryFrom<u16> for RpuWireOp {
    type Error = RpuWireError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Status),
            1 => Ok(Self::ReadRegister),
            2 => Ok(Self::WriteRegister),
            3 => Ok(Self::ReadAttenuation),
            4 => Ok(Self::SetAttenuation),
            5 => Ok(Self::ReadBias),
            6 => Ok(Self::SetBias),
            7 => Ok(Self::ReadTrim),
            8 => Ok(Self::SetTrim),
            9 => Ok(Self::ReadOffset),
            10 => Ok(Self::SetOffset),
            11 => Ok(Self::ReadVbiasControl),
            12 => Ok(Self::SetVbiasControl),
            13 => Ok(Self::SetReset),
            14 => Ok(Self::DoReset),
            15 => Ok(Self::SetPowerState),
            16 => Ok(Self::Align),
            17 => Ok(Self::BeginConfigureFrontend),
            18 => Ok(Self::ConfigureAfe),
            19 => Ok(Self::ConfigureChannel),
            20 => Ok(Self::ApplyConfigureFrontend),
            21 => Ok(Self::WriteFunction),
            22 => Ok(Self::ReadAlignment),
            _ => Err(RpuWireError::UnknownOp(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RpuWireTarget {
    None = 0,
    Afe = 1,
    Channel = 2,
    All = 3,
}

impl TryFrom<u8> for RpuWireTarget {
    type Error = RpuWireError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Afe),
            2 => Ok(Self::Channel),
            3 => Ok(Self::All),
            _ => Err(RpuWireError::UnknownTarget(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum RpuWireStatus {
    Accepted = 0,
    Applied = 1,
    Rejected = 2,
    Interlocked = 3,
    Fault = 4,
    Timeout = 5,
}

impl TryFrom<u16> for RpuWireStatus {
    type Error = RpuWireError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Accepted),
            1 => Ok(Self::Applied),
            2 => Ok(Self::Rejected),
            3 => Ok(Self::Interlocked),
            4 => Ok(Self::Fault),
            5 => Ok(Self::Timeout),
            _ => Err(RpuWireError::UnknownStatus(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpuWireCommand {
    pub sequence: u64,
    pub op: RpuWireOp,
    pub target: RpuWireTarget,
    pub afe_board: u8,
    pub afe_pl: u8,
    pub channel: u8,
    pub afe_channel: u8,
    pub register: u16,
    pub value: u32,
    pub flags: u32,
    pub payload: [u8; RPU_WIRE_PAYLOAD_LEN],
}

impl RpuWireCommand {
    pub fn status(sequence: u64) -> Self {
        Self {
            sequence,
            op: RpuWireOp::Status,
            target: RpuWireTarget::None,
            afe_board: 0,
            afe_pl: 0,
            channel: 0,
            afe_channel: 0,
            register: 0,
            value: 0,
            flags: 0,
            payload: [0; RPU_WIRE_PAYLOAD_LEN],
        }
    }

    pub fn frame_count_for_afe_command(command: &AfeCommand) -> Result<usize, RpuWireError> {
        match command {
            AfeCommand::ConfigureFrontend { afes, channels, .. } => {
                ensure_record_count("AFE config", afes.len())?;
                ensure_record_count("channel config", channels.len())?;
                Ok(2 + afes.len() + channels.len())
            }
            _ => Ok(1),
        }
    }

    pub fn from_afe_command_sequence(
        first_sequence: u64,
        command: &AfeCommand,
    ) -> Result<Vec<Self>, RpuWireError> {
        match command {
            AfeCommand::ConfigureFrontend {
                afes,
                channels,
                bias_control,
            } => Self::configure_frontend_sequence(first_sequence, afes, channels, *bias_control),
            _ => Ok(vec![Self::from_afe_command(first_sequence, command)?]),
        }
    }

    pub fn from_afe_command(sequence: u64, command: &AfeCommand) -> Result<Self, RpuWireError> {
        let mut wire = Self::status(sequence);

        match command {
            AfeCommand::ReadRegister { afe, register } => {
                wire.op = RpuWireOp::ReadRegister;
                wire.set_afe(*afe);
                wire.register = u16::from(*register);
            }
            AfeCommand::WriteRegister {
                afe,
                register,
                value,
            } => {
                wire.op = RpuWireOp::WriteRegister;
                wire.set_afe(*afe);
                wire.register = u16::from(*register);
                wire.value = u32::from(*value);
            }
            AfeCommand::ReadAttenuation { afe } => {
                wire.op = RpuWireOp::ReadAttenuation;
                wire.set_afe(*afe);
            }
            AfeCommand::SetAttenuation { afe, value } => {
                wire.op = RpuWireOp::SetAttenuation;
                wire.set_afe(*afe);
                wire.value = u32::from(*value);
            }
            AfeCommand::ReadBias { afe } => {
                wire.op = RpuWireOp::ReadBias;
                wire.set_afe(*afe);
            }
            AfeCommand::SetBias { afe, value } => {
                wire.op = RpuWireOp::SetBias;
                wire.set_afe(*afe);
                wire.value = u32::from(*value);
            }
            AfeCommand::ReadTrim { target } => {
                wire.op = RpuWireOp::ReadTrim;
                wire.set_channel_target(target.clone());
            }
            AfeCommand::SetTrim {
                target,
                value,
                gain,
            } => {
                wire.op = RpuWireOp::SetTrim;
                wire.set_channel_target(target.clone());
                wire.value = u32::from(*value);
                wire.flags = u32::from(*gain);
            }
            AfeCommand::ReadOffset { target } => {
                wire.op = RpuWireOp::ReadOffset;
                wire.set_channel_target(target.clone());
            }
            AfeCommand::SetOffset {
                target,
                value,
                gain,
            } => {
                wire.op = RpuWireOp::SetOffset;
                wire.set_channel_target(target.clone());
                wire.value = u32::from(*value);
                wire.flags = u32::from(*gain);
            }
            AfeCommand::ReadVbiasControl => {
                wire.op = RpuWireOp::ReadVbiasControl;
            }
            AfeCommand::SetVbiasControl { value, enable } => {
                wire.op = RpuWireOp::SetVbiasControl;
                wire.value = u32::from(*value);
                wire.flags = u32::from(*enable);
            }
            AfeCommand::SetReset { asserted } => {
                wire.op = RpuWireOp::SetReset;
                wire.flags = u32::from(*asserted);
            }
            AfeCommand::DoReset => {
                wire.op = RpuWireOp::DoReset;
            }
            AfeCommand::SetPowerState { enabled } => {
                wire.op = RpuWireOp::SetPowerState;
                wire.flags = u32::from(*enabled);
            }
            AfeCommand::Align => {
                wire.op = RpuWireOp::Align;
            }
            AfeCommand::WriteFunction { afe, name, value } => {
                wire.op = RpuWireOp::WriteFunction;
                wire.set_afe(*afe);
                wire.value = u32::from(*value);
                set_function_name(&mut wire.payload, name)?;
            }
            AfeCommand::ReadAlignment { afe } => {
                wire.op = RpuWireOp::ReadAlignment;
                wire.set_afe(*afe);
            }
            AfeCommand::ConfigureFrontend { .. } => {
                return Err(RpuWireError::UnsupportedCommand(
                    "configure-frontend requires a multi-frame RPU config sequence",
                ));
            }
        }

        Ok(wire)
    }

    fn configure_frontend_sequence(
        first_sequence: u64,
        afes: &[AfeFrontendConfig],
        channels: &[ChannelFrontendConfig],
        bias_control: u16,
    ) -> Result<Vec<Self>, RpuWireError> {
        ensure_record_count("AFE config", afes.len())?;
        ensure_record_count("channel config", channels.len())?;

        let mut frames = Vec::with_capacity(2 + afes.len() + channels.len());
        let mut begin = Self::status(sequence_for_frame(first_sequence, frames.len()));
        begin.op = RpuWireOp::BeginConfigureFrontend;
        begin.value = u32::from(bias_control);
        begin.flags = pack_u16_pair(afes.len() as u16, channels.len() as u16);
        frames.push(begin);

        for afe in afes {
            let mut frame = Self::status(sequence_for_frame(first_sequence, frames.len()));
            frame.op = RpuWireOp::ConfigureAfe;
            frame.set_afe(afe.afe);
            frame.value = u32::from(afe.attenuation);
            frame.flags = u32::from(afe.bias);
            pack_afe_payload(&mut frame.payload, afe);
            frames.push(frame);
        }

        for channel in channels {
            let mut frame = Self::status(sequence_for_frame(first_sequence, frames.len()));
            frame.op = RpuWireOp::ConfigureChannel;
            frame.set_channel(channel.channel);
            frame.value = u32::from(channel.trim);
            frame.flags = pack_u16_pair(channel.offset, channel.gain);
            frames.push(frame);
        }

        let mut apply = Self::status(sequence_for_frame(first_sequence, frames.len()));
        apply.op = RpuWireOp::ApplyConfigureFrontend;
        apply.flags = pack_u16_pair(afes.len() as u16, channels.len() as u16);
        frames.push(apply);

        Ok(frames)
    }

    pub fn encode(&self) -> [u8; RPU_WIRE_COMMAND_LEN] {
        let mut out = [0_u8; RPU_WIRE_COMMAND_LEN];
        put_u32(&mut out, 0, RPU_WIRE_MAGIC);
        put_u16(&mut out, 4, RPU_WIRE_ABI_VERSION);
        put_u16(&mut out, 6, self.op as u16);
        put_u64(&mut out, 8, self.sequence);
        out[16] = self.target as u8;
        out[17] = self.afe_board;
        out[18] = self.afe_pl;
        out[19] = self.channel;
        out[20] = self.afe_channel;
        put_u16(&mut out, 22, self.register);
        put_u32(&mut out, 24, self.value);
        put_u32(&mut out, 28, self.flags);
        out[32..64].copy_from_slice(&self.payload);
        out
    }

    pub fn decode(input: &[u8]) -> Result<Self, RpuWireError> {
        if input.len() != RPU_WIRE_COMMAND_LEN {
            return Err(RpuWireError::BadLength {
                expected: RPU_WIRE_COMMAND_LEN,
                actual: input.len(),
            });
        }
        let magic = get_u32(input, 0);
        if magic != RPU_WIRE_MAGIC {
            return Err(RpuWireError::BadMagic(magic));
        }
        let abi = get_u16(input, 4);
        if abi != RPU_WIRE_ABI_VERSION {
            return Err(RpuWireError::BadAbi(abi));
        }

        Ok(Self {
            sequence: get_u64(input, 8),
            op: RpuWireOp::try_from(get_u16(input, 6))?,
            target: RpuWireTarget::try_from(input[16])?,
            afe_board: input[17],
            afe_pl: input[18],
            channel: input[19],
            afe_channel: input[20],
            register: get_u16(input, 22),
            value: get_u32(input, 24),
            flags: get_u32(input, 28),
            payload: input[32..64]
                .try_into()
                .expect("payload slice length is fixed"),
        })
    }

    pub fn configure_counts(&self) -> Result<RpuWireConfigCounts, RpuWireError> {
        require_op(self.op, RpuWireOp::BeginConfigureFrontend)?;
        let (afe_count, channel_count) = unpack_u16_pair(self.flags);
        Ok(RpuWireConfigCounts {
            afe_count,
            channel_count,
            bias_control: require_u16("bias_control", self.value)?,
        })
    }

    pub fn afe_config(&self) -> Result<RpuWireAfeConfig, RpuWireError> {
        require_op(self.op, RpuWireOp::ConfigureAfe)?;
        Ok(RpuWireAfeConfig {
            afe_board: self.afe_board,
            afe_pl: self.afe_pl,
            attenuation: require_u16("attenuation", self.value)?,
            bias: require_u16("bias", self.flags)?,
            adc: unpack_adc(self.payload[0]),
            pga: unpack_pga(self.payload[1], self.payload[2]),
            lna: unpack_lna(self.payload[3], self.payload[4], self.payload[5]),
        })
    }

    pub fn channel_config(&self) -> Result<RpuWireChannelConfig, RpuWireError> {
        require_op(self.op, RpuWireOp::ConfigureChannel)?;
        let (offset, gain) = unpack_u16_pair(self.flags);
        Ok(RpuWireChannelConfig {
            channel: self.channel,
            afe_board: self.afe_board,
            afe_pl: self.afe_pl,
            afe_channel: self.afe_channel,
            trim: require_u16("trim", self.value)?,
            offset,
            gain,
        })
    }

    pub fn function_name(&self) -> Result<&str, RpuWireError> {
        require_op(self.op, RpuWireOp::WriteFunction)?;
        let length = usize::from(self.payload[0]);
        if length == 0 || length > RPU_WIRE_FUNCTION_NAME_MAX {
            return Err(RpuWireError::BadPayload("invalid AFE function name length"));
        }
        str::from_utf8(&self.payload[1..1 + length])
            .map_err(|_| RpuWireError::BadPayload("AFE function name is not UTF-8"))
    }

    fn set_afe(&mut self, afe: AfeId) {
        self.target = RpuWireTarget::Afe;
        self.afe_board = afe.board();
        self.afe_pl = afe.pl();
    }

    fn set_channel_target(&mut self, target: ChannelTarget) {
        match target {
            ChannelTarget::One(channel) => {
                self.set_channel(channel);
            }
            ChannelTarget::Afe(afe) => {
                self.set_afe(afe);
            }
            ChannelTarget::All => {
                self.target = RpuWireTarget::All;
            }
        }
    }

    fn set_channel(&mut self, channel: ChannelId) {
        self.target = RpuWireTarget::Channel;
        self.channel = channel.channel();
        self.afe_board = channel.afe().board();
        self.afe_pl = channel.afe().pl();
        self.afe_channel = channel.afe_channel();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpuWireReply {
    pub sequence: u64,
    pub status: RpuWireStatus,
    pub readback: Option<u32>,
    pub heartbeat: u64,
    pub fault_code: u32,
    pub interlock_code: u32,
}

impl RpuWireReply {
    pub fn encode(&self) -> [u8; RPU_WIRE_REPLY_LEN] {
        let mut out = [0_u8; RPU_WIRE_REPLY_LEN];
        put_u32(&mut out, 0, RPU_WIRE_MAGIC);
        put_u16(&mut out, 4, RPU_WIRE_ABI_VERSION);
        put_u16(&mut out, 6, self.status as u16);
        put_u64(&mut out, 8, self.sequence);
        put_u32(&mut out, 16, self.readback.unwrap_or_default());
        put_u32(&mut out, 20, u32::from(self.readback.is_some()));
        put_u64(&mut out, 24, self.heartbeat);
        put_u32(&mut out, 32, self.fault_code);
        put_u32(&mut out, 36, self.interlock_code);
        out
    }

    pub fn decode(input: &[u8]) -> Result<Self, RpuWireError> {
        if input.len() != RPU_WIRE_REPLY_LEN {
            return Err(RpuWireError::BadLength {
                expected: RPU_WIRE_REPLY_LEN,
                actual: input.len(),
            });
        }
        let magic = get_u32(input, 0);
        if magic != RPU_WIRE_MAGIC {
            return Err(RpuWireError::BadMagic(magic));
        }
        let abi = get_u16(input, 4);
        if abi != RPU_WIRE_ABI_VERSION {
            return Err(RpuWireError::BadAbi(abi));
        }
        let readback = if get_u32(input, 20) == 0 {
            None
        } else {
            Some(get_u32(input, 16))
        };
        Ok(Self {
            sequence: get_u64(input, 8),
            status: RpuWireStatus::try_from(get_u16(input, 6))?,
            readback,
            heartbeat: get_u64(input, 24),
            fault_code: get_u32(input, 32),
            interlock_code: get_u32(input, 36),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpuWireConfigCounts {
    pub afe_count: u16,
    pub channel_count: u16,
    pub bias_control: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpuWireAfeConfig {
    pub afe_board: u8,
    pub afe_pl: u8,
    pub attenuation: u16,
    pub bias: u16,
    pub adc: AdcConfig,
    pub pga: PgaConfig,
    pub lna: LnaConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpuWireChannelConfig {
    pub channel: u8,
    pub afe_board: u8,
    pub afe_pl: u8,
    pub afe_channel: u8,
    pub trim: u16,
    pub offset: u16,
    pub gain: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpuWireError {
    BadLength {
        expected: usize,
        actual: usize,
    },
    BadMagic(u32),
    BadAbi(u16),
    UnknownOp(u16),
    UnknownTarget(u8),
    UnknownStatus(u16),
    UnsupportedCommand(&'static str),
    BadPayload(&'static str),
    PayloadTooLong {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    ValueOutOfRange {
        field: &'static str,
        max: u32,
        actual: u32,
    },
}

impl fmt::Display for RpuWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength { expected, actual } => {
                write!(f, "bad RPU wire length: expected {expected}, got {actual}")
            }
            Self::BadMagic(magic) => write!(f, "bad RPU wire magic: 0x{magic:08X}"),
            Self::BadAbi(abi) => write!(f, "unsupported RPU ABI version: {abi}"),
            Self::UnknownOp(op) => write!(f, "unknown RPU wire operation: {op}"),
            Self::UnknownTarget(target) => write!(f, "unknown RPU wire target: {target}"),
            Self::UnknownStatus(status) => write!(f, "unknown RPU wire status: {status}"),
            Self::UnsupportedCommand(message) => write!(f, "{message}"),
            Self::BadPayload(message) => write!(f, "bad RPU wire payload: {message}"),
            Self::PayloadTooLong { field, max, actual } => {
                write!(f, "{field} is too long: {actual}; max {max}")
            }
            Self::ValueOutOfRange { field, max, actual } => {
                write!(f, "{field} is out of range: {actual}; max {max}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RpuWireError {}

fn put_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn sequence_for_frame(first_sequence: u64, offset: usize) -> u64 {
    first_sequence.wrapping_add(offset as u64).max(1)
}

fn ensure_record_count(field: &'static str, count: usize) -> Result<(), RpuWireError> {
    if count > u16::MAX as usize {
        return Err(RpuWireError::PayloadTooLong {
            field,
            max: u16::MAX as usize,
            actual: count,
        });
    }
    Ok(())
}

fn require_op(actual: RpuWireOp, expected: RpuWireOp) -> Result<(), RpuWireError> {
    if actual == expected {
        Ok(())
    } else {
        Err(RpuWireError::BadPayload(
            "decoded helper used with wrong opcode",
        ))
    }
}

fn require_u16(field: &'static str, value: u32) -> Result<u16, RpuWireError> {
    u16::try_from(value).map_err(|_| RpuWireError::ValueOutOfRange {
        field,
        max: u32::from(u16::MAX),
        actual: value,
    })
}

fn pack_u16_pair(low: u16, high: u16) -> u32 {
    u32::from(low) | (u32::from(high) << 16)
}

fn unpack_u16_pair(value: u32) -> (u16, u16) {
    ((value & 0xFFFF) as u16, (value >> 16) as u16)
}

fn set_function_name(
    payload: &mut [u8; RPU_WIRE_PAYLOAD_LEN],
    name: &str,
) -> Result<(), RpuWireError> {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return Err(RpuWireError::BadPayload("AFE function name is empty"));
    }
    if bytes.len() > RPU_WIRE_FUNCTION_NAME_MAX {
        return Err(RpuWireError::PayloadTooLong {
            field: "AFE function name",
            max: RPU_WIRE_FUNCTION_NAME_MAX,
            actual: bytes.len(),
        });
    }
    payload[0] = bytes.len() as u8;
    payload[1..1 + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

fn pack_afe_payload(payload: &mut [u8; RPU_WIRE_PAYLOAD_LEN], afe: &AfeFrontendConfig) {
    payload[0] = pack_adc(afe.adc);
    payload[1] = afe.pga.lpf_cut_frequency;
    payload[2] = pack_pga_flags(afe.pga);
    payload[3] = afe.lna.clamp;
    payload[4] = afe.lna.gain;
    payload[5] = u8::from(afe.lna.integrator_disable);
}

fn pack_adc(adc: AdcConfig) -> u8 {
    u8::from(adc.resolution) | (u8::from(adc.output_format) << 1) | (u8::from(adc.msb_first) << 2)
}

fn unpack_adc(flags: u8) -> AdcConfig {
    AdcConfig {
        resolution: flags & 0x01 != 0,
        output_format: flags & 0x02 != 0,
        msb_first: flags & 0x04 != 0,
    }
}

fn pack_pga_flags(pga: PgaConfig) -> u8 {
    u8::from(pga.integrator_disable) | (u8::from(pga.gain) << 1)
}

fn unpack_pga(lpf_cut_frequency: u8, flags: u8) -> PgaConfig {
    PgaConfig {
        lpf_cut_frequency,
        integrator_disable: flags & 0x01 != 0,
        gain: flags & 0x02 != 0,
    }
}

fn unpack_lna(clamp: u8, gain: u8, flags: u8) -> LnaConfig {
    LnaConfig {
        clamp,
        gain,
        integrator_disable: flags & 0x01 != 0,
    }
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([input[offset], input[offset + 1]])
}

fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::afe::{
        AdcConfig, AfeCommand, AfeFrontendConfig, AfeId, ChannelFrontendConfig, ChannelId,
        ChannelTarget, LnaConfig, PgaConfig,
    };

    #[test]
    fn encodes_write_register_command() {
        let afe = AfeId::from_board(1).unwrap();
        let command = AfeCommand::WriteRegister {
            afe,
            register: 3,
            value: 0x1234,
        };

        let wire = RpuWireCommand::from_afe_command(99, &command).unwrap();
        let decoded = RpuWireCommand::decode(&wire.encode()).unwrap();

        assert_eq!(decoded.sequence, 99);
        assert_eq!(decoded.op, RpuWireOp::WriteRegister);
        assert_eq!(decoded.target, RpuWireTarget::Afe);
        assert_eq!(decoded.afe_board, 1);
        assert_eq!(decoded.afe_pl, 4);
        assert_eq!(decoded.register, 3);
        assert_eq!(decoded.value, 0x1234);
    }

    #[test]
    fn encodes_channel_target() {
        let channel = ChannelId::new(15).unwrap();
        let command = AfeCommand::SetTrim {
            target: ChannelTarget::One(channel),
            value: 55,
            gain: true,
        };

        let wire = RpuWireCommand::from_afe_command(1, &command).unwrap();

        assert_eq!(wire.target, RpuWireTarget::Channel);
        assert_eq!(wire.channel, 15);
        assert_eq!(wire.afe_board, 1);
        assert_eq!(wire.afe_channel, 7);
        assert_eq!(wire.value, 55);
        assert_eq!(wire.flags, 1);
    }

    #[test]
    fn reply_roundtrips() {
        let reply = RpuWireReply {
            sequence: 10,
            status: RpuWireStatus::Applied,
            readback: Some(0xDEAD_BEEF),
            heartbeat: 42,
            fault_code: 0,
            interlock_code: 0,
        };

        assert_eq!(RpuWireReply::decode(&reply.encode()).unwrap(), reply);
    }

    #[test]
    fn encodes_write_function_name() {
        let command = AfeCommand::WriteFunction {
            afe: AfeId::from_board(0).unwrap(),
            name: "adc_reset".to_string(),
            value: 1,
        };

        let wire = RpuWireCommand::from_afe_command(1, &command).unwrap();
        let decoded = RpuWireCommand::decode(&wire.encode()).unwrap();

        assert_eq!(decoded.op, RpuWireOp::WriteFunction);
        assert_eq!(decoded.function_name().unwrap(), "adc_reset");
        assert_eq!(decoded.value, 1);
    }

    #[test]
    fn rejects_overlong_write_function_name() {
        let command = AfeCommand::WriteFunction {
            afe: AfeId::from_board(0).unwrap(),
            name: "x".repeat(RPU_WIRE_FUNCTION_NAME_MAX + 1),
            value: 1,
        };

        let err = RpuWireCommand::from_afe_command(1, &command).unwrap_err();

        assert!(matches!(err, RpuWireError::PayloadTooLong { .. }));
    }

    #[test]
    fn encodes_configure_frontend_sequence() {
        let afe = AfeId::from_board(1).unwrap();
        let channel = ChannelId::new(10).unwrap();
        let command = AfeCommand::ConfigureFrontend {
            afes: vec![AfeFrontendConfig {
                afe,
                attenuation: 100,
                bias: 200,
                adc: AdcConfig {
                    resolution: true,
                    output_format: false,
                    msb_first: true,
                },
                pga: PgaConfig {
                    lpf_cut_frequency: 3,
                    integrator_disable: true,
                    gain: false,
                },
                lna: LnaConfig {
                    clamp: 4,
                    gain: 5,
                    integrator_disable: true,
                },
            }],
            channels: vec![ChannelFrontendConfig {
                channel,
                trim: 300,
                offset: 400,
                gain: 500,
            }],
            bias_control: 600,
        };

        let frames = RpuWireCommand::from_afe_command_sequence(20, &command).unwrap();

        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].op, RpuWireOp::BeginConfigureFrontend);
        assert_eq!(frames[0].sequence, 20);
        assert_eq!(
            frames[0].configure_counts().unwrap(),
            RpuWireConfigCounts {
                afe_count: 1,
                channel_count: 1,
                bias_control: 600
            }
        );

        assert_eq!(frames[1].op, RpuWireOp::ConfigureAfe);
        assert_eq!(frames[1].sequence, 21);
        assert_eq!(
            frames[1].afe_config().unwrap(),
            RpuWireAfeConfig {
                afe_board: 1,
                afe_pl: 4,
                attenuation: 100,
                bias: 200,
                adc: AdcConfig {
                    resolution: true,
                    output_format: false,
                    msb_first: true,
                },
                pga: PgaConfig {
                    lpf_cut_frequency: 3,
                    integrator_disable: true,
                    gain: false,
                },
                lna: LnaConfig {
                    clamp: 4,
                    gain: 5,
                    integrator_disable: true,
                },
            }
        );

        assert_eq!(frames[2].op, RpuWireOp::ConfigureChannel);
        assert_eq!(frames[2].sequence, 22);
        assert_eq!(
            frames[2].channel_config().unwrap(),
            RpuWireChannelConfig {
                channel: 10,
                afe_board: 1,
                afe_pl: 4,
                afe_channel: 2,
                trim: 300,
                offset: 400,
                gain: 500,
            }
        );
        assert_eq!(frames[3].op, RpuWireOp::ApplyConfigureFrontend);
        assert_eq!(frames[3].sequence, 23);
    }
}
