use crate::afe::{AfeCommand, AfeId, ChannelId, ChannelTarget};
use core::fmt;

pub const RPU_WIRE_MAGIC: u32 = 0x5250_5344; // "DSPR" little-endian marker
pub const RPU_WIRE_ABI_VERSION: u16 = 1;
pub const RPU_WIRE_COMMAND_LEN: usize = 64;
pub const RPU_WIRE_REPLY_LEN: usize = 64;

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
            AfeCommand::WriteFunction { .. } => {
                return Err(RpuWireError::UnsupportedCommand(
                    "AFE function writes require a string dictionary in the RPU ABI",
                ));
            }
            AfeCommand::ConfigureFrontend { .. } => {
                return Err(RpuWireError::UnsupportedCommand(
                    "configure-frontend requires the chunked RPU config protocol",
                ));
            }
        }

        Ok(wire)
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
        })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpuWireError {
    BadLength { expected: usize, actual: usize },
    BadMagic(u32),
    BadAbi(u16),
    UnknownOp(u16),
    UnknownTarget(u8),
    UnknownStatus(u16),
    UnsupportedCommand(&'static str),
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
    use crate::afe::{AfeCommand, AfeId, ChannelId, ChannelTarget};

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
    fn rejects_variable_length_commands_until_chunked_protocol_exists() {
        let command = AfeCommand::WriteFunction {
            afe: AfeId::from_board(0).unwrap(),
            name: "x".to_string(),
            value: 1,
        };

        let err = RpuWireCommand::from_afe_command(1, &command).unwrap_err();

        assert!(matches!(err, RpuWireError::UnsupportedCommand(_)));
    }
}
