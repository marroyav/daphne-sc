#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfeBitField {
    pub register: u16,
    pub msb: u8,
    pub lsb: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfeFunctionValues {
    Range { min: u16, max: u16 },
    Options(&'static [u16]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfeFunctionSpec {
    pub name: &'static str,
    pub field: AfeBitField,
    pub values: AfeFunctionValues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfeFunctionError {
    UnknownName,
    InvalidValue,
    InvalidBitField,
}

macro_rules! afe_function {
    ($name:literal, $register:literal, $msb:literal, $lsb:literal, range $min:literal, $max:literal) => {
        AfeFunctionSpec {
            name: $name,
            field: AfeBitField {
                register: $register,
                msb: $msb,
                lsb: $lsb,
            },
            values: AfeFunctionValues::Range {
                min: $min,
                max: $max,
            },
        }
    };
    ($name:literal, $register:literal, $msb:literal, $lsb:literal, options [$($value:literal),+ $(,)?]) => {
        AfeFunctionSpec {
            name: $name,
            field: AfeBitField {
                register: $register,
                msb: $msb,
                lsb: $lsb,
            },
            values: AfeFunctionValues::Options(&[$($value),+]),
        }
    };
}

pub const AFE_FUNCTIONS: &[AfeFunctionSpec] = &[
    afe_function!("SOFTWARE_RESET", 0, 0, 0, range 0, 1),
    afe_function!("REGISTER_READOUT_ENABLE", 0, 1, 1, range 0, 1),
    afe_function!("ADC_COMPLETE_PDN", 1, 0, 0, range 0, 1),
    afe_function!("LVDS_OUTPUT_DISABLE", 1, 1, 1, range 0, 1),
    afe_function!("ADC_PDN_CH", 1, 9, 2, range 0, 0xFF),
    afe_function!("PARTIAL_PDN", 1, 10, 10, range 0, 1),
    afe_function!("LOW_FREQUENCY_NOISE_SUPPRESSION", 1, 11, 11, range 0, 1),
    afe_function!("EXT_REF", 1, 13, 13, range 0, 1),
    afe_function!("LVDS_OUTPUT_RATE_2X", 1, 12, 14, range 0, 1),
    afe_function!("SINGLE-ENDED_CLK_MODE", 1, 15, 15, range 0, 1),
    afe_function!("POWER-DOWN_LVDS", 2, 10, 3, range 0, 1),
    afe_function!("AVERAGING_ENABLE", 2, 11, 11, range 0, 1),
    afe_function!("LOW_LATENCY", 2, 12, 12, range 0, 1),
    afe_function!("TEST_PATTERN_MODES", 2, 15, 13, range 0, 0x7),
    afe_function!("INVERT_CHANNELS", 3, 7, 0, range 0, 0xFF),
    afe_function!("CHANNEL_OFFSET_SUBSTRACTION_ENABLE", 3, 8, 8, range 0, 1),
    afe_function!("DIGITAL_GAIN_ENABLE", 3, 12, 12, range 0, 1),
    afe_function!("SERIALIZED_DATA_RATE", 3, 14, 13, range 0, 0x3),
    afe_function!("ENABLE_EXTERNAL_REFERENCE_MODE", 3, 15, 15, range 0, 1),
    afe_function!("ADC_RESOLUTION_RESET", 4, 1, 1, range 0, 1),
    afe_function!("ADC_OUTPUT_FORMAT", 4, 3, 3, range 0, 1),
    afe_function!("LSB_MSB_FIRST", 4, 4, 4, range 0, 1),
    afe_function!("CUSTOM_PATTERN", 5, 13, 0, range 0, 0x3FFF),
    afe_function!("SYNC_PATTERN", 10, 8, 8, range 0, 1),
    afe_function!("OFFSET_CH1", 13, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH1", 13, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH2", 15, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH2", 15, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH3", 17, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH3", 17, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH4", 19, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH4", 19, 15, 11, range 0, 0x1F),
    afe_function!("DIGITAL_HPF_FILTER_ENABLE_CH1-4", 21, 0, 0, range 0, 1),
    afe_function!("DIGITAL_HPF_FILTER_K_CH1-4", 21, 4, 1, range 2, 10),
    afe_function!("OFFSET_CH8", 25, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH8", 25, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH7", 27, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH7", 27, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH6", 29, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH6", 29, 15, 11, range 0, 0x1F),
    afe_function!("OFFSET_CH5", 31, 9, 0, range 0, 0x3FF),
    afe_function!("DIGITAL_GAIN_CH5", 31, 15, 11, range 0, 0x1F),
    afe_function!("DIGITAL_HPF_FILTER_ENABLE_CH5-8", 33, 0, 0, range 0, 1),
    afe_function!("DIGITAL_HPF_FILTER_K_CH5-8", 33, 4, 1, range 2, 10),
    afe_function!("DITHER", 66, 15, 15, range 0, 1),
    afe_function!("PGA_CLAMP_-6dB", 50, 10, 10, range 0, 1),
    afe_function!("LPF_PROGRAMMABILITY", 51, 3, 1, options [0, 2, 3, 4]),
    afe_function!("PGA_INTEGRATOR_DISABLE", 51, 4, 4, range 0, 1),
    afe_function!("PGA_CLAMP_LEVEL", 51, 7, 5, range 0, 7),
    afe_function!("PGA_GAIN_CONTROL", 51, 13, 13, range 0, 1),
    afe_function!(
        "ACTIVE_TERMINATION_INDIVIDUAL_RESISTOR_CNTL",
        52,
        4,
        0,
        range 0,
        0x1F
    ),
    afe_function!(
        "ACTIVE_TERMINATION_INDIVIDUAL_RESISTOR_ENABLE",
        52,
        5,
        5,
        range 0,
        1
    ),
    afe_function!("PRESET_ACTIVE_TERMINATIONS", 52, 7, 6, range 0, 3),
    afe_function!("ACTIVE_TERMINATION_ENABLE", 52, 8, 8, range 0, 1),
    afe_function!("LNA_INPUT_CLAMP_SETTING", 52, 10, 9, range 0, 3),
    afe_function!("LNA_INTEGRATOR_DISABLE", 52, 12, 12, range 0, 1),
    afe_function!("LNA_GAIN", 52, 14, 13, range 0, 3),
    afe_function!("LNA_INDIVIDUAL_CH_CNTL", 52, 15, 15, range 0, 1),
    afe_function!("PDN_CH", 53, 7, 0, range 0, 0xFF),
    afe_function!("LOW_POWER", 53, 10, 10, range 0, 1),
    afe_function!("MED_POWER", 53, 11, 11, range 0, 1),
    afe_function!("PDN_VCAT_PGA", 53, 12, 12, range 0, 1),
    afe_function!("PDN_LNA", 53, 13, 13, range 0, 1),
    afe_function!("VCA_PARTIAL_PDN", 53, 14, 14, range 0, 1),
    afe_function!("VCA_COMPLETE_PDN", 53, 15, 15, range 0, 1),
    afe_function!(
        "CW_SUM_AMP_GAIN_CNTL",
        54,
        4,
        0,
        options [0, 1, 2, 4, 8, 16]
    ),
    afe_function!("CW_16X_CLK_SEL", 54, 5, 5, range 0, 1),
    afe_function!("CW_1X_CLK_SEL", 54, 6, 6, range 0, 1),
    afe_function!("CW_TGC_SEL", 54, 8, 8, range 0, 1),
    afe_function!("CW_SUM_AMP_ENABLE", 54, 9, 9, range 0, 1),
    afe_function!("CW_CLK_MODE_SEL", 54, 11, 10, range 0, 3),
    afe_function!("CH1_CW_MIXER_PHASE", 55, 3, 0, range 0, 0xF),
    afe_function!("CH2_CW_MIXER_PHASE", 55, 7, 4, range 0, 0xF),
    afe_function!("CH3_CW_MIXER_PHASE", 55, 11, 8, range 0, 0xF),
    afe_function!("CH4_CW_MIXER_PHASE", 55, 15, 12, range 0, 0xF),
    afe_function!("CH5_CW_MIXER_PHASE", 56, 3, 0, range 0, 0xF),
    afe_function!("CH6_CW_MIXER_PHASE", 56, 7, 4, range 0, 0xF),
    afe_function!("CH7_CW_MIXER_PHASE", 56, 11, 8, range 0, 0xF),
    afe_function!("CH8_CW_MIXER_PHASE", 56, 15, 12, range 0, 0xF),
    afe_function!("CH1_LNA_GAIN_CNTL", 57, 1, 0, range 0, 3),
    afe_function!("CH2_LNA_GAIN_CNTL", 57, 3, 2, range 0, 3),
    afe_function!("CH3_LNA_GAIN_CNTL", 57, 5, 4, range 0, 3),
    afe_function!("CH4_LNA_GAIN_CNTL", 57, 7, 6, range 0, 3),
    afe_function!("CH5_LNA_GAIN_CNTL", 57, 9, 8, range 0, 3),
    afe_function!("CH6_LNA_GAIN_CNTL", 57, 11, 10, range 0, 3),
    afe_function!("CH7_LNA_GAIN_CNTL", 57, 13, 12, range 0, 3),
    afe_function!("CH8_LNA_GAIN_CNTL", 57, 15, 14, range 0, 3),
    afe_function!("HPF_LNA", 59, 3, 2, range 0, 3),
    afe_function!("DIG_TGC_ATT_GAIN", 59, 6, 4, range 0, 0x7),
    afe_function!("DIG_TGC_ATT", 59, 7, 7, range 0, 1),
    afe_function!("CW_SUM_AMP_PDN", 59, 8, 8, range 0, 1),
    afe_function!("PGA_TEST_MODE", 59, 9, 9, range 0, 1),
];

impl AfeFunctionValues {
    pub fn contains(self, value: u16) -> bool {
        match self {
            AfeFunctionValues::Range { min, max } => value >= min && value <= max,
            AfeFunctionValues::Options(options) => options.contains(&value),
        }
    }
}

impl AfeBitField {
    pub fn mask(self) -> Result<u16, AfeFunctionError> {
        if self.msb < self.lsb || self.msb >= 16 {
            return Err(AfeFunctionError::InvalidBitField);
        }

        let width = self.msb - self.lsb + 1;
        let mask = ((1_u32 << width) - 1) << self.lsb;
        Ok(mask as u16)
    }
}

pub fn find_afe_function(name: &str) -> Option<&'static AfeFunctionSpec> {
    AFE_FUNCTIONS.iter().find(|spec| spec.name == name)
}

pub fn validate_afe_function_value(
    name: &str,
    value: u16,
) -> Result<&'static AfeFunctionSpec, AfeFunctionError> {
    let spec = find_afe_function(name).ok_or(AfeFunctionError::UnknownName)?;
    if spec.values.contains(value) {
        Ok(spec)
    } else {
        Err(AfeFunctionError::InvalidValue)
    }
}

pub fn replace_afe_function_bits(
    register_value: u16,
    field: AfeBitField,
    value: u16,
) -> Result<u16, AfeFunctionError> {
    let mask = field.mask()?;
    Ok((register_value & !mask) | ((value << field.lsb) & mask))
}

pub fn extract_afe_function_bits(
    register_value: u16,
    field: AfeBitField,
) -> Result<u16, AfeFunctionError> {
    let mask = field.mask()?;
    Ok((register_value & mask) >> field.lsb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_legacy_function_fields() {
        let spec = find_afe_function("PGA_CLAMP_LEVEL").unwrap();

        assert_eq!(spec.field.register, 51);
        assert_eq!(spec.field.msb, 7);
        assert_eq!(spec.field.lsb, 5);
        assert!(matches!(
            spec.values,
            AfeFunctionValues::Range { min: 0, max: 7 }
        ));
    }

    #[test]
    fn validates_ranges_and_explicit_options_like_cpp_server() {
        assert!(validate_afe_function_value("DIGITAL_HPF_FILTER_K_CH1-4", 2).is_ok());
        assert!(validate_afe_function_value("DIGITAL_HPF_FILTER_K_CH1-4", 10).is_ok());
        assert_eq!(
            validate_afe_function_value("DIGITAL_HPF_FILTER_K_CH1-4", 1),
            Err(AfeFunctionError::InvalidValue)
        );

        assert!(validate_afe_function_value("LPF_PROGRAMMABILITY", 4).is_ok());
        assert_eq!(
            validate_afe_function_value("LPF_PROGRAMMABILITY", 1),
            Err(AfeFunctionError::InvalidValue)
        );
        assert_eq!(
            validate_afe_function_value("NOT_A_FUNCTION", 0),
            Err(AfeFunctionError::UnknownName)
        );
    }

    #[test]
    fn replaces_and_extracts_function_bits_without_touching_neighbors() {
        let spec = find_afe_function("LPF_PROGRAMMABILITY").unwrap();
        let updated = replace_afe_function_bits(0xFFFF, spec.field, 4).unwrap();

        assert_eq!(updated, 0xFFF9);
        assert_eq!(extract_afe_function_bits(updated, spec.field), Ok(4));
    }

    #[test]
    fn rejects_malformed_legacy_bitfield_instead_of_shifting_invalid_width() {
        let spec = find_afe_function("LVDS_OUTPUT_RATE_2X").unwrap();

        assert_eq!(
            replace_afe_function_bits(0, spec.field, 1),
            Err(AfeFunctionError::InvalidBitField)
        );
    }
}
