#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageTypeV2 {
    ConfigureClksReq = 200,
    ConfigureFeReq = 202,
    WriteAfeRegReq = 204,
    WriteAfeVgainReq = 206,
    WriteAfeBiasSetReq = 208,
    WriteTrimAllChReq = 210,
    WriteTrimAllAfeReq = 212,
    WriteTrimChReq = 214,
    WriteOffsetAllChReq = 216,
    WriteOffsetAllAfeReq = 218,
    WriteOffsetChReq = 220,
    WriteVbiasControlReq = 222,
    ReadAfeRegReq = 224,
    ReadAfeVgainReq = 226,
    ReadAfeBiasSetReq = 228,
    ReadTrimAllChReq = 230,
    ReadTrimAllAfeReq = 232,
    ReadTrimChReq = 234,
    ReadOffsetAllChReq = 236,
    ReadOffsetAllAfeReq = 238,
    ReadOffsetChReq = 240,
    ReadVbiasControlReq = 242,
    ReadCurrentMonitorReq = 244,
    ReadBiasVoltageMonitorReq = 246,
    SetAfeResetReq = 248,
    DoAfeResetReq = 250,
    SetAfePowerStateReq = 252,
    WriteAfeAttenuationReq = 254,
    DumpSpybufferReq = 256,
    AlignAfeReq = 258,
    WriteAfeFunctionReq = 260,
    DoSoftwareTriggerReq = 262,
    DumpSpybufferChunkReq = 300,
    ReadTestRegReq = 304,
    ReadTriggerCountersReq = 320,
    ReadGeneralInfoReq = 322,
    ReadSlowControlStatusReq = 1000,
}

impl TryFrom<u32> for MessageTypeV2 {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            200 => Ok(Self::ConfigureClksReq),
            202 => Ok(Self::ConfigureFeReq),
            204 => Ok(Self::WriteAfeRegReq),
            206 => Ok(Self::WriteAfeVgainReq),
            208 => Ok(Self::WriteAfeBiasSetReq),
            210 => Ok(Self::WriteTrimAllChReq),
            212 => Ok(Self::WriteTrimAllAfeReq),
            214 => Ok(Self::WriteTrimChReq),
            216 => Ok(Self::WriteOffsetAllChReq),
            218 => Ok(Self::WriteOffsetAllAfeReq),
            220 => Ok(Self::WriteOffsetChReq),
            222 => Ok(Self::WriteVbiasControlReq),
            224 => Ok(Self::ReadAfeRegReq),
            226 => Ok(Self::ReadAfeVgainReq),
            228 => Ok(Self::ReadAfeBiasSetReq),
            230 => Ok(Self::ReadTrimAllChReq),
            232 => Ok(Self::ReadTrimAllAfeReq),
            234 => Ok(Self::ReadTrimChReq),
            236 => Ok(Self::ReadOffsetAllChReq),
            238 => Ok(Self::ReadOffsetAllAfeReq),
            240 => Ok(Self::ReadOffsetChReq),
            242 => Ok(Self::ReadVbiasControlReq),
            244 => Ok(Self::ReadCurrentMonitorReq),
            246 => Ok(Self::ReadBiasVoltageMonitorReq),
            248 => Ok(Self::SetAfeResetReq),
            250 => Ok(Self::DoAfeResetReq),
            252 => Ok(Self::SetAfePowerStateReq),
            254 => Ok(Self::WriteAfeAttenuationReq),
            256 => Ok(Self::DumpSpybufferReq),
            258 => Ok(Self::AlignAfeReq),
            260 => Ok(Self::WriteAfeFunctionReq),
            262 => Ok(Self::DoSoftwareTriggerReq),
            300 => Ok(Self::DumpSpybufferChunkReq),
            304 => Ok(Self::ReadTestRegReq),
            320 => Ok(Self::ReadTriggerCountersReq),
            322 => Ok(Self::ReadGeneralInfoReq),
            1000 => Ok(Self::ReadSlowControlStatusReq),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRoute {
    RpuAfe,
    LinuxStatus,
    LinuxFpga,
    Unsupported,
}

pub fn route_message_type(message_type: MessageTypeV2) -> CommandRoute {
    use MessageTypeV2::*;

    match message_type {
        ConfigureFeReq
        | WriteAfeRegReq
        | WriteAfeVgainReq
        | WriteAfeBiasSetReq
        | WriteTrimAllChReq
        | WriteTrimAllAfeReq
        | WriteTrimChReq
        | WriteOffsetAllChReq
        | WriteOffsetAllAfeReq
        | WriteOffsetChReq
        | WriteVbiasControlReq
        | ReadAfeRegReq
        | ReadAfeVgainReq
        | ReadAfeBiasSetReq
        | ReadTrimAllChReq
        | ReadTrimAllAfeReq
        | ReadTrimChReq
        | ReadOffsetAllChReq
        | ReadOffsetAllAfeReq
        | ReadOffsetChReq
        | ReadVbiasControlReq
        | SetAfeResetReq
        | DoAfeResetReq
        | SetAfePowerStateReq
        | WriteAfeAttenuationReq
        | AlignAfeReq
        | WriteAfeFunctionReq => CommandRoute::RpuAfe,

        ReadCurrentMonitorReq
        | ReadBiasVoltageMonitorReq
        | ReadGeneralInfoReq
        | ReadSlowControlStatusReq => CommandRoute::LinuxStatus,

        ConfigureClksReq
        | DumpSpybufferReq
        | DumpSpybufferChunkReq
        | DoSoftwareTriggerReq
        | ReadTriggerCountersReq
        | ReadTestRegReq => CommandRoute::LinuxFpga,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afe_writes_route_to_rpu() {
        assert_eq!(
            route_message_type(MessageTypeV2::WriteAfeRegReq),
            CommandRoute::RpuAfe
        );
        assert_eq!(
            route_message_type(MessageTypeV2::WriteTrimChReq),
            CommandRoute::RpuAfe
        );
        assert_eq!(
            route_message_type(MessageTypeV2::ConfigureFeReq),
            CommandRoute::RpuAfe
        );
    }

    #[test]
    fn non_afe_status_stays_on_linux() {
        assert_eq!(
            route_message_type(MessageTypeV2::ReadGeneralInfoReq),
            CommandRoute::LinuxStatus
        );
    }
}
