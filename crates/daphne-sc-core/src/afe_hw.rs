use crate::afe::AFE_COUNT;

pub const FPGA_REGISTER_BASE: u64 = 0x8000_0000;

pub const AFE_GLOBAL_CONTROL_OFFSET: u32 = 0x0000_0000;
pub const AFE_CONTROL_BASE_OFFSET: u32 = 0x0000_0004;
pub const AFE_DAC_TRIM_BASE_OFFSET: u32 = 0x0000_0008;
pub const AFE_DAC_OFFSET_BASE_OFFSET: u32 = 0x0000_000C;
pub const AFE_REGISTER_STRIDE: u32 = 0x0000_000C;

pub const DAC_GAIN_BIAS_CONTROL_OFFSET: u32 = 0x0C00_0000;
pub const DAC_GAIN_BIAS_U50_OFFSET: u32 = 0x0C00_0004;
pub const DAC_GAIN_BIAS_U53_OFFSET: u32 = 0x0C00_0008;
pub const DAC_GAIN_BIAS_U5_OFFSET: u32 = 0x0C00_000C;
pub const BIAS_ENABLE_OFFSET: u32 = 0x1400_000C;

pub const AFE_GLOBAL_RESET_BIT: u8 = 0;
pub const AFE_GLOBAL_POWERSTATE_BIT: u8 = 1;
pub const AFE_GLOBAL_BUSY_BITS: core::ops::RangeInclusive<u8> = 2..=4;

pub const AFE_SPI_TRIGGER_WORD: u32 = 0x0000_0002;
pub const AFE_SPI_IDLE_WORD: u32 = 0x0000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DacChip {
    U50,
    U53,
    U5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DacRoute {
    pub chip: DacChip,
    pub channel: u8,
    pub gain: bool,
    pub buffer: bool,
}

pub const AFE_GAIN_DAC_ROUTES: [DacRoute; AFE_COUNT as usize] = [
    DacRoute::new(DacChip::U50, 0),
    DacRoute::new(DacChip::U50, 1),
    DacRoute::new(DacChip::U50, 2),
    DacRoute::new(DacChip::U50, 3),
    DacRoute::new(DacChip::U5, 1),
];

pub const AFE_BIAS_DAC_ROUTES: [DacRoute; AFE_COUNT as usize] = [
    DacRoute::new(DacChip::U53, 0),
    DacRoute::new(DacChip::U53, 1),
    DacRoute::new(DacChip::U53, 2),
    DacRoute::new(DacChip::U53, 3),
    DacRoute::new(DacChip::U5, 0),
];

pub const VBIAS_DAC_ROUTE: DacRoute = DacRoute::new(DacChip::U5, 2);

impl DacRoute {
    pub const fn new(chip: DacChip, channel: u8) -> Self {
        Self {
            chip,
            channel,
            gain: false,
            buffer: false,
        }
    }
}

pub const fn afe_control_offset(afe_pl: u8) -> Option<u32> {
    afe_register_offset(AFE_CONTROL_BASE_OFFSET, afe_pl)
}

pub const fn afe_dac_trim_offset(afe_pl: u8) -> Option<u32> {
    afe_register_offset(AFE_DAC_TRIM_BASE_OFFSET, afe_pl)
}

pub const fn afe_dac_offset_offset(afe_pl: u8) -> Option<u32> {
    afe_register_offset(AFE_DAC_OFFSET_BASE_OFFSET, afe_pl)
}

pub const fn dac_gain_bias_offset(chip: DacChip) -> u32 {
    match chip {
        DacChip::U50 => DAC_GAIN_BIAS_U50_OFFSET,
        DacChip::U53 => DAC_GAIN_BIAS_U53_OFFSET,
        DacChip::U5 => DAC_GAIN_BIAS_U5_OFFSET,
    }
}

pub const fn afe_register_word(register: u16, value: u16) -> u32 {
    ((register as u32 & 0xFF) << 16) | value as u32
}

pub const fn afe_register_address_word(register: u16) -> u32 {
    (register as u32 & 0xFF) << 16
}

pub const fn dac_gain_bias_word(route: DacRoute, value: u16) -> u32 {
    ((route.channel as u32 & 0x03) << 14)
        | ((route.gain as u32) << 13)
        | ((route.buffer as u32) << 12)
        | (value as u32 & 0x0FFF)
}

pub const fn dac_trim_offset_half(channel: u8, value: u16, gain: bool, buffer: bool) -> u16 {
    ((channel as u16 & 0x03) << 14)
        | ((gain as u16) << 13)
        | ((buffer as u16) << 12)
        | (value & 0x0FFF)
}

pub const fn dac_pair_word(low_half: u16, high_half: u16) -> u32 {
    ((high_half as u32) << 16) | low_half as u32
}

pub const fn dac_companion_channel(channel: u8) -> Option<u8> {
    if channel < 4 {
        Some(channel + 4)
    } else if channel < 8 {
        Some(channel - 4)
    } else {
        None
    }
}

pub const fn dac_channel_is_high_half(channel: u8) -> bool {
    channel >= 4
}

const fn afe_register_offset(base: u32, afe_pl: u8) -> Option<u32> {
    if afe_pl < AFE_COUNT {
        Some(base + AFE_REGISTER_STRIDE * afe_pl as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afe_register_offsets_match_cpp_and_firmware_maps() {
        assert_eq!(afe_control_offset(0), Some(0x04));
        assert_eq!(afe_dac_trim_offset(0), Some(0x08));
        assert_eq!(afe_dac_offset_offset(0), Some(0x0C));
        assert_eq!(afe_control_offset(4), Some(0x34));
        assert_eq!(afe_dac_trim_offset(4), Some(0x38));
        assert_eq!(afe_dac_offset_offset(4), Some(0x3C));
        assert_eq!(afe_control_offset(5), None);
    }

    #[test]
    fn packs_afe_register_words_like_cpp_server() {
        assert_eq!(afe_register_word(0x03, 0x1234), 0x0003_1234);
        assert_eq!(afe_register_address_word(0x03), 0x0003_0000);
        assert_eq!(AFE_SPI_TRIGGER_WORD, 0x0000_0002);
    }

    #[test]
    fn gain_bias_routes_match_cpp_server() {
        assert_eq!(AFE_GAIN_DAC_ROUTES[4], DacRoute::new(DacChip::U5, 1));
        assert_eq!(AFE_BIAS_DAC_ROUTES[4], DacRoute::new(DacChip::U5, 0));
        assert_eq!(dac_gain_bias_offset(DacChip::U50), 0x0C00_0004);
        assert_eq!(dac_gain_bias_offset(DacChip::U53), 0x0C00_0008);
        assert_eq!(dac_gain_bias_offset(DacChip::U5), 0x0C00_000C);
        assert_eq!(BIAS_ENABLE_OFFSET, 0x1400_000C);
        assert_eq!(VBIAS_DAC_ROUTE, DacRoute::new(DacChip::U5, 2));
    }

    #[test]
    fn packs_dac_words_like_cpp_server() {
        let route = DacRoute {
            chip: DacChip::U50,
            channel: 2,
            gain: true,
            buffer: true,
        };
        assert_eq!(dac_gain_bias_word(route, 0x0ABC), 0x0000_BABC);
        assert_eq!(dac_trim_offset_half(6, 0x0555, true, false), 0xA555);
        assert_eq!(dac_pair_word(0x1111, 0x2222), 0x2222_1111);
        assert_eq!(dac_companion_channel(2), Some(6));
        assert_eq!(dac_companion_channel(6), Some(2));
        assert_eq!(dac_companion_channel(8), None);
        assert!(!dac_channel_is_high_half(3));
        assert!(dac_channel_is_high_half(4));
    }
}
