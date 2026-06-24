use core::fmt;

pub const AFE_COUNT: u8 = 5;
pub const CHANNELS_PER_AFE: u8 = 8;
pub const CHANNEL_COUNT: u8 = AFE_COUNT * CHANNELS_PER_AFE;
pub const DAC_12BIT_MAX: u16 = 4095;

pub const AFE_BOARD_TO_PL: [u8; AFE_COUNT as usize] = [0, 4, 3, 2, 1];
pub const AFE_PL_TO_BOARD: [u8; AFE_COUNT as usize] = [0, 4, 3, 2, 1];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfeId {
    board: u8,
    pl: u8,
}

impl AfeId {
    pub fn from_board(board: u8) -> Result<Self, AfeValidationError> {
        let pl = *AFE_BOARD_TO_PL
            .get(board as usize)
            .ok_or(AfeValidationError::AfeOutOfRange { afe: board })?;
        Ok(Self { board, pl })
    }

    pub const fn board(self) -> u8 {
        self.board
    }

    pub const fn pl(self) -> u8 {
        self.pl
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelId {
    channel: u8,
    afe: AfeId,
    afe_channel: u8,
}

impl ChannelId {
    pub fn new(channel: u8) -> Result<Self, AfeValidationError> {
        if channel >= CHANNEL_COUNT {
            return Err(AfeValidationError::ChannelOutOfRange { channel });
        }

        let board_afe = channel / CHANNELS_PER_AFE;
        let afe_channel = channel % CHANNELS_PER_AFE;
        Ok(Self {
            channel,
            afe: AfeId::from_board(board_afe)?,
            afe_channel,
        })
    }

    pub const fn channel(self) -> u8 {
        self.channel
    }

    pub const fn afe(self) -> AfeId {
        self.afe
    }

    pub const fn afe_channel(self) -> u8 {
        self.afe_channel
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfeTarget {
    One(AfeId),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelTarget {
    One(ChannelId),
    Afe(AfeId),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfeCommand {
    ReadRegister {
        afe: AfeId,
        register: u8,
    },
    WriteRegister {
        afe: AfeId,
        register: u8,
        value: u16,
    },
    ReadAttenuation {
        afe: AfeId,
    },
    SetAttenuation {
        afe: AfeId,
        value: u16,
    },
    ReadBias {
        afe: AfeId,
    },
    SetBias {
        afe: AfeId,
        value: u16,
    },
    ReadTrim {
        target: ChannelTarget,
    },
    SetTrim {
        target: ChannelTarget,
        value: u16,
        gain: bool,
    },
    ReadOffset {
        target: ChannelTarget,
    },
    SetOffset {
        target: ChannelTarget,
        value: u16,
        gain: bool,
    },
    ReadVbiasControl,
    SetVbiasControl {
        value: u16,
        enable: bool,
    },
    SetReset {
        asserted: bool,
    },
    DoReset,
    SetPowerState {
        enabled: bool,
    },
    WriteFunction {
        afe: AfeId,
        name: String,
        value: u16,
    },
    ConfigureFrontend {
        afes: Vec<AfeFrontendConfig>,
        channels: Vec<ChannelFrontendConfig>,
        bias_control: u16,
    },
    Align,
}

impl AfeCommand {
    pub fn validate(&self) -> Result<(), AfeValidationError> {
        match self {
            Self::SetAttenuation { value, .. }
            | Self::SetBias { value, .. }
            | Self::SetTrim { value, .. }
            | Self::SetOffset { value, .. }
            | Self::SetVbiasControl { value, .. } => validate_12bit(*value),
            Self::ConfigureFrontend {
                afes,
                channels,
                bias_control,
            } => {
                validate_12bit(*bias_control)?;
                for afe in afes {
                    validate_12bit(afe.attenuation)?;
                    validate_12bit(afe.bias)?;
                }
                for channel in channels {
                    validate_12bit(channel.trim)?;
                    validate_12bit(channel.offset)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfeFrontendConfig {
    pub afe: AfeId,
    pub attenuation: u16,
    pub bias: u16,
    pub adc: AdcConfig,
    pub pga: PgaConfig,
    pub lna: LnaConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelFrontendConfig {
    pub channel: ChannelId,
    pub trim: u16,
    pub offset: u16,
    pub gain: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdcConfig {
    pub resolution: bool,
    pub output_format: bool,
    pub msb_first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgaConfig {
    pub lpf_cut_frequency: u8,
    pub integrator_disable: bool,
    pub gain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LnaConfig {
    pub clamp: u8,
    pub gain: u8,
    pub integrator_disable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfeValidationError {
    AfeOutOfRange { afe: u8 },
    ChannelOutOfRange { channel: u8 },
    ValueOutOfRange { value: u16, max: u16 },
}

impl fmt::Display for AfeValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfeOutOfRange { afe } => write!(f, "AFE out of range: {afe}; expected 0..4"),
            Self::ChannelOutOfRange { channel } => {
                write!(f, "channel out of range: {channel}; expected 0..39")
            }
            Self::ValueOutOfRange { value, max } => {
                write!(f, "value out of range: {value}; expected 0..{max}")
            }
        }
    }
}

impl std::error::Error for AfeValidationError {}

fn validate_12bit(value: u16) -> Result<(), AfeValidationError> {
    if value > DAC_12BIT_MAX {
        return Err(AfeValidationError::ValueOutOfRange {
            value,
            max: DAC_12BIT_MAX,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_to_pl_mapping_matches_cpp_server() {
        let expected = [0, 4, 3, 2, 1];
        for (board, pl) in expected.iter().enumerate() {
            assert_eq!(AfeId::from_board(board as u8).unwrap().pl(), *pl);
        }
    }

    #[test]
    fn channel_maps_to_board_afe_then_pl_afe() {
        let channel = ChannelId::new(15).unwrap();
        assert_eq!(channel.afe().board(), 1);
        assert_eq!(channel.afe().pl(), 4);
        assert_eq!(channel.afe_channel(), 7);
    }

    #[test]
    fn rejects_out_of_range_dac_value() {
        let afe = AfeId::from_board(0).unwrap();
        let cmd = AfeCommand::SetBias { afe, value: 4096 };
        assert!(matches!(
            cmd.validate(),
            Err(AfeValidationError::ValueOutOfRange { .. })
        ));
    }
}
