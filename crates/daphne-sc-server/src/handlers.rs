use crate::pb;
use crate::preflight::PreflightStatus;
use crate::v2;
use daphne_sc_core::afe::{
    AdcConfig, AfeCommand, AfeFrontendConfig, AfeId, ChannelFrontendConfig, ChannelId,
    ChannelTarget, LnaConfig, PgaConfig,
};
use daphne_sc_core::rpu::{AfeCommandReply, RpuAfeTransport, RpuError, RpuLinkStatus};
use daphne_sc_core::transport::{route_message_type, CommandRoute, MessageTypeV2};
use prost::Message;

const RPU_FAIL_PREFIX: &str = "RPU AFE command failed: ";

pub fn handle_payload<T: RpuAfeTransport>(
    message_type: i32,
    payload: &[u8],
    rpu: &mut T,
    preflight: &PreflightStatus,
) -> Vec<u8> {
    let Ok(core_type) = MessageTypeV2::try_from(message_type as u32) else {
        return Vec::new();
    };

    match route_message_type(core_type) {
        CommandRoute::RpuAfe if preflight.ready_for_hardware_commands() => {
            handle_rpu_afe(core_type, payload, rpu)
        }
        CommandRoute::RpuAfe => {
            let mut rejected = PreflightRejectedRpuTransport::new(preflight.failure_message());
            handle_rpu_afe(core_type, payload, &mut rejected)
        }
        CommandRoute::LinuxStatus => handle_linux_status(core_type, payload),
        CommandRoute::LinuxFpga | CommandRoute::Unsupported => handle_not_implemented(core_type),
    }
}

struct PreflightRejectedRpuTransport {
    message: String,
}

impl PreflightRejectedRpuTransport {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl RpuAfeTransport for PreflightRejectedRpuTransport {
    fn status(&mut self) -> Result<RpuLinkStatus, RpuError> {
        Ok(RpuLinkStatus {
            available: false,
            running: false,
            firmware: None,
            heartbeat: None,
            last_fault: Some(self.message.clone()),
        })
    }

    fn submit_afe_command(&mut self, _command: AfeCommand) -> Result<AfeCommandReply, RpuError> {
        Err(RpuError::Rejected(self.message.clone()))
    }
}

fn handle_rpu_afe<T: RpuAfeTransport>(
    message_type: MessageTypeV2,
    payload: &[u8],
    rpu: &mut T,
) -> Vec<u8> {
    match message_type {
        MessageTypeV2::ConfigureFeReq => handle_configure_fe(payload, rpu),
        MessageTypeV2::WriteAfeRegReq => handle_write_afe_reg(payload, rpu),
        MessageTypeV2::WriteAfeVgainReq => handle_write_afe_vgain(payload, rpu),
        MessageTypeV2::WriteAfeBiasSetReq => handle_write_afe_bias(payload, rpu),
        MessageTypeV2::WriteTrimAllChReq => handle_write_trim_all_channels(payload, rpu),
        MessageTypeV2::WriteTrimAllAfeReq => handle_write_trim_all_afe(payload, rpu),
        MessageTypeV2::WriteTrimChReq => handle_write_trim_channel(payload, rpu),
        MessageTypeV2::WriteOffsetAllChReq => handle_write_offset_all_channels(payload, rpu),
        MessageTypeV2::WriteOffsetAllAfeReq => handle_write_offset_all_afe(payload, rpu),
        MessageTypeV2::WriteOffsetChReq => handle_write_offset_channel(payload, rpu),
        MessageTypeV2::WriteVbiasControlReq => handle_write_vbias_control(payload, rpu),
        MessageTypeV2::ReadAfeRegReq => handle_read_afe_reg(payload, rpu),
        MessageTypeV2::ReadAfeVgainReq => handle_read_afe_vgain(payload, rpu),
        MessageTypeV2::ReadAfeBiasSetReq => handle_read_afe_bias(payload, rpu),
        MessageTypeV2::ReadTrimAllChReq => handle_read_trim_all_channels(payload, rpu),
        MessageTypeV2::ReadTrimAllAfeReq => handle_read_trim_all_afe(payload, rpu),
        MessageTypeV2::ReadTrimChReq => handle_read_trim_channel(payload, rpu),
        MessageTypeV2::ReadOffsetAllChReq => handle_read_offset_all_channels(payload, rpu),
        MessageTypeV2::ReadOffsetAllAfeReq => handle_read_offset_all_afe(payload, rpu),
        MessageTypeV2::ReadOffsetChReq => handle_read_offset_channel(payload, rpu),
        MessageTypeV2::ReadVbiasControlReq => handle_read_vbias_control(payload, rpu),
        MessageTypeV2::SetAfeResetReq => handle_set_afe_reset(payload, rpu),
        MessageTypeV2::DoAfeResetReq => handle_do_afe_reset(payload, rpu),
        MessageTypeV2::SetAfePowerStateReq => handle_set_afe_power_state(payload, rpu),
        MessageTypeV2::WriteAfeAttenuationReq => handle_write_afe_attenuation(payload, rpu),
        MessageTypeV2::AlignAfeReq => handle_align_afe(payload, rpu),
        MessageTypeV2::WriteAfeFunctionReq => handle_write_afe_function(payload, rpu),
        _ => Vec::new(),
    }
}

fn handle_linux_status(message_type: MessageTypeV2, payload: &[u8]) -> Vec<u8> {
    match message_type {
        MessageTypeV2::ReadCurrentMonitorReq => {
            let req = match decode::<pb::CmdReadCurrentMonitor>(payload) {
                Ok(req) => req,
                Err(err) => {
                    return v2::encode(pb::CmdReadCurrentMonitorResponse {
                        success: false,
                        message: format!("Bad cmd_readCurrentMonitor payload: {err}"),
                        ..Default::default()
                    });
                }
            };
            v2::encode(pb::CmdReadCurrentMonitorResponse {
                success: false,
                message: "Current monitor not implemented in Rust server yet".to_string(),
                current_monitor_channel: req.current_monitor_channel,
                current_value: 0,
            })
        }
        MessageTypeV2::ReadBiasVoltageMonitorReq => {
            let req = match decode::<pb::CmdReadBiasVoltageMonitor>(payload) {
                Ok(req) => req,
                Err(err) => {
                    return v2::encode(pb::CmdReadBiasVoltageMonitorResponse {
                        success: false,
                        message: format!("Bad cmd_readBiasVoltageMonitor payload: {err}"),
                        ..Default::default()
                    });
                }
            };
            v2::encode(pb::CmdReadBiasVoltageMonitorResponse {
                success: false,
                message: "Bias voltage monitor not implemented in Rust server yet".to_string(),
                afe_block: req.afe_block,
                bias_voltage_value: 0,
            })
        }
        MessageTypeV2::ReadGeneralInfoReq => v2::encode(pb::GeneralInfo::default()),
        _ => Vec::new(),
    }
}

fn handle_not_implemented(message_type: MessageTypeV2) -> Vec<u8> {
    match message_type {
        MessageTypeV2::ReadTestRegReq => v2::encode(pb::TestRegResponse {
            value: 0xDEADBEEF,
            message: "ok".to_string(),
        }),
        _ => Vec::new(),
    }
}

fn handle_configure_fe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::ConfigureRequest>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::ConfigureResponse {
                success: false,
                message: format!("Bad ConfigureRequest payload: {err}"),
            });
        }
    };

    let command = match configure_command(req) {
        Ok(command) => command,
        Err(err) => {
            return v2::encode(pb::ConfigureResponse {
                success: false,
                message: err,
            });
        }
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::ConfigureResponse {
        success: outcome.success,
        message: outcome.message,
    })
}

fn handle_write_afe_reg<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteAfeReg>(payload) {
        Ok(req) => req,
        Err(err) => return bad_write_afe_reg(format!("Bad cmd_writeAFEReg payload: {err}")),
    };

    let command = match make_write_register(req.afe_block, req.reg_address, req.reg_value) {
        Ok(command) => command,
        Err(err) => return bad_write_afe_reg(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteAfeRegResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        reg_address: req.reg_address,
        reg_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_write_afe_reg(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteAfeRegResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_afe_vgain<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteAfevgain>(payload) {
        Ok(req) => req,
        Err(err) => return bad_write_afe_vgain(format!("Bad cmd_writeAFEVGAIN payload: {err}")),
    };

    let command = match make_set_attenuation(req.afe_block, req.vgain_value) {
        Ok(command) => command,
        Err(err) => return bad_write_afe_vgain(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteAfevgainResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        vgain_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_write_afe_vgain(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteAfevgainResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_afe_attenuation<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteAfeAttenuation>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_afe_attenuation(format!(
                "Bad cmd_writeAFEAttenuation payload: {err}"
            ));
        }
    };

    let command = match make_set_attenuation(req.afe_block, req.attenuation) {
        Ok(command) => command,
        Err(err) => return bad_write_afe_attenuation(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteAfeAttenuationResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        attenuation: outcome.readback.unwrap_or_default(),
    })
}

fn bad_write_afe_attenuation(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteAfeAttenuationResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_afe_bias<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteAfeBiasSet>(payload) {
        Ok(req) => req,
        Err(err) => return bad_write_afe_bias(format!("Bad cmd_writeAFEBiasSet payload: {err}")),
    };

    let command = match make_set_bias(req.afe_block, req.bias_value) {
        Ok(command) => command,
        Err(err) => return bad_write_afe_bias(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteAfeBiasSetResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        bias_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_write_afe_bias(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteAfeBiasSetResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_trim_channel<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteTrimSingleChannel>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_trim_channel(format!(
                "Bad cmd_writeTrim_singleChannel payload: {err}"
            ));
        }
    };

    let target = match channel_id(req.trim_channel).map(ChannelTarget::One) {
        Ok(target) => target,
        Err(err) => return bad_write_trim_channel(err),
    };
    let command = match make_set_trim(target, req.trim_value, req.trim_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_trim_channel(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteTrimSingleChannelResponse {
        success: outcome.success,
        message: outcome.message,
        trim_channel: req.trim_channel,
        trim_value: outcome.readback.unwrap_or_default(),
        trim_gain: req.trim_gain,
    })
}

fn bad_write_trim_channel(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteTrimSingleChannelResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_trim_all_channels<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteTrimAllChannels>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_trim_all_channels(format!(
                "Bad cmd_writeTRIM_allChannels payload: {err}"
            ));
        }
    };

    let command = match make_set_trim(ChannelTarget::All, req.trim_value, req.trim_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_trim_all_channels(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteTrimAllChannelsResponse {
        success: outcome.success,
        message: outcome.message,
        trim_value: outcome.readback.unwrap_or_default(),
        trim_gain: req.trim_gain,
    })
}

fn bad_write_trim_all_channels(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteTrimAllChannelsResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_trim_all_afe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteTrimAllAfe>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_trim_all_afe(format!("Bad cmd_writeTrim_allAFE payload: {err}"));
        }
    };

    let target = match afe_id(req.afe_block).map(ChannelTarget::Afe) {
        Ok(target) => target,
        Err(err) => return bad_write_trim_all_afe(err),
    };
    let command = match make_set_trim(target, req.trim_value, req.trim_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_trim_all_afe(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteTrimAllAfeResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        trim_value: outcome.readback.unwrap_or_default(),
        trim_gain: req.trim_gain,
    })
}

fn bad_write_trim_all_afe(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteTrimAllAfeResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_offset_channel<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteOffsetSingleChannel>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_offset_channel(format!(
                "Bad cmd_writeOFFSET_singleChannel payload: {err}"
            ));
        }
    };

    let target = match channel_id(req.offset_channel).map(ChannelTarget::One) {
        Ok(target) => target,
        Err(err) => return bad_write_offset_channel(err),
    };
    let command = match make_set_offset(target, req.offset_value, req.offset_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_offset_channel(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteOffsetSingleChannelResponse {
        success: outcome.success,
        message: outcome.message,
        offset_channel: req.offset_channel,
        offset_value: outcome.readback.unwrap_or_default(),
        offset_gain: req.offset_gain,
    })
}

fn bad_write_offset_channel(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteOffsetSingleChannelResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_offset_all_channels<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteOffsetAllChannels>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_offset_all_channels(format!(
                "Bad cmd_writeOFFSET_allChannels payload: {err}"
            ));
        }
    };

    let command = match make_set_offset(ChannelTarget::All, req.offset_value, req.offset_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_offset_all_channels(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteOffsetAllChannelsResponse {
        success: outcome.success,
        message: outcome.message,
        offset_value: outcome.readback.unwrap_or_default(),
        offset_gain: req.offset_gain,
    })
}

fn bad_write_offset_all_channels(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteOffsetAllChannelsResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_offset_all_afe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteOffsetAllAfe>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_offset_all_afe(format!("Bad cmd_writeOFFSET_allAFE payload: {err}"));
        }
    };

    let target = match afe_id(req.afe_block).map(ChannelTarget::Afe) {
        Ok(target) => target,
        Err(err) => return bad_write_offset_all_afe(err),
    };
    let command = match make_set_offset(target, req.offset_value, req.offset_gain) {
        Ok(command) => command,
        Err(err) => return bad_write_offset_all_afe(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteOffsetAllAfeResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        offset_value: outcome.readback.unwrap_or_default(),
        offset_gain: req.offset_gain,
    })
}

fn bad_write_offset_all_afe(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteOffsetAllAfeResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_write_vbias_control<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteVbiasControl>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_write_vbias_control(format!("Bad cmd_writeVbiasControl payload: {err}"));
        }
    };

    let command = match make_set_vbias_control(req.v_bias_control_value, req.enable) {
        Ok(command) => command,
        Err(err) => return bad_write_vbias_control(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteVbiasControlResponse {
        success: outcome.success,
        message: outcome.message,
        v_bias_control_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_write_vbias_control(message: String) -> Vec<u8> {
    v2::encode(pb::CmdWriteVbiasControlResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_afe_reg<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadAfeReg>(payload) {
        Ok(req) => req,
        Err(err) => return bad_read_afe_reg(format!("Bad cmd_readAFEReg payload: {err}")),
    };

    let command = match make_read_register(req.afe_block, req.reg_address) {
        Ok(command) => command,
        Err(err) => return bad_read_afe_reg(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadAfeRegResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        reg_address: req.reg_address,
        reg_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_read_afe_reg(message: String) -> Vec<u8> {
    v2::encode(pb::CmdReadAfeRegResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_afe_vgain<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadAfeVgain>(payload) {
        Ok(req) => req,
        Err(err) => return bad_read_afe_vgain(format!("Bad cmd_readAFEVgain payload: {err}")),
    };

    let command = match afe_id(req.afe_block).map(|afe| AfeCommand::ReadAttenuation { afe }) {
        Ok(command) => command,
        Err(err) => return bad_read_afe_vgain(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadAfeVgainResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        vgain_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_read_afe_vgain(message: String) -> Vec<u8> {
    v2::encode(pb::CmdReadAfeVgainResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_afe_bias<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadAfeBiasSet>(payload) {
        Ok(req) => req,
        Err(err) => return bad_read_afe_bias(format!("Bad cmd_readAFEBiasSet payload: {err}")),
    };

    let command = match afe_id(req.afe_block).map(|afe| AfeCommand::ReadBias { afe }) {
        Ok(command) => command,
        Err(err) => return bad_read_afe_bias(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadAfeBiasSetResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        bias_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_read_afe_bias(message: String) -> Vec<u8> {
    v2::encode(pb::CmdReadAfeBiasSetResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_trim_channel<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadTrimSingleChannel>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_read_trim_channel(format!("Bad cmd_readTrim_singleChannel payload: {err}"));
        }
    };

    let command = match channel_id(req.trim_channel).map(|channel| AfeCommand::ReadTrim {
        target: ChannelTarget::One(channel),
    }) {
        Ok(command) => command,
        Err(err) => return bad_read_trim_channel(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadTrimSingleChannelResponse {
        success: outcome.success,
        message: outcome.message,
        trim_channel: req.trim_channel,
        trim_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_read_trim_channel(message: String) -> Vec<u8> {
    v2::encode(pb::CmdReadTrimSingleChannelResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_trim_all_channels<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    if let Err(err) = decode::<pb::CmdReadTrimAllChannels>(payload) {
        return v2::encode(pb::CmdReadTrimAllChannelsResponse {
            success: false,
            message: format!("Bad cmd_readTrim_allChannels payload: {err}"),
            ..Default::default()
        });
    }

    let outcome = submit(
        rpu,
        AfeCommand::ReadTrim {
            target: ChannelTarget::All,
        },
    );
    v2::encode(pb::CmdReadTrimAllChannelsResponse {
        success: outcome.success,
        message: outcome.message,
        trim_values: Vec::new(),
    })
}

fn handle_read_trim_all_afe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadTrimAllAfe>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::CmdReadTrimAllAfeResponse {
                success: false,
                message: format!("Bad cmd_readTrim_allAFE payload: {err}"),
                ..Default::default()
            });
        }
    };

    let command = match afe_id(req.afe_block).map(|afe| AfeCommand::ReadTrim {
        target: ChannelTarget::Afe(afe),
    }) {
        Ok(command) => command,
        Err(err) => {
            return v2::encode(pb::CmdReadTrimAllAfeResponse {
                success: false,
                message: err,
                afe_block: req.afe_block,
                ..Default::default()
            });
        }
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadTrimAllAfeResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        trim_values: Vec::new(),
    })
}

fn handle_read_offset_channel<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadOffsetSingleChannel>(payload) {
        Ok(req) => req,
        Err(err) => {
            return bad_read_offset_channel(format!(
                "Bad cmd_readOffset_singleChannel payload: {err}"
            ));
        }
    };

    let command = match channel_id(req.offset_channel).map(|channel| AfeCommand::ReadOffset {
        target: ChannelTarget::One(channel),
    }) {
        Ok(command) => command,
        Err(err) => return bad_read_offset_channel(err),
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadOffsetSingleChannelResponse {
        success: outcome.success,
        message: outcome.message,
        offset_channel: req.offset_channel,
        offset_value: outcome.readback.unwrap_or_default(),
    })
}

fn bad_read_offset_channel(message: String) -> Vec<u8> {
    v2::encode(pb::CmdReadOffsetSingleChannelResponse {
        success: false,
        message,
        ..Default::default()
    })
}

fn handle_read_offset_all_channels<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    if let Err(err) = decode::<pb::CmdReadOffsetAllChannels>(payload) {
        return v2::encode(pb::CmdReadOffsetAllChannelsResponse {
            success: false,
            message: format!("Bad cmd_readOffset_allChannels payload: {err}"),
            ..Default::default()
        });
    }

    let outcome = submit(
        rpu,
        AfeCommand::ReadOffset {
            target: ChannelTarget::All,
        },
    );
    v2::encode(pb::CmdReadOffsetAllChannelsResponse {
        success: outcome.success,
        message: outcome.message,
        offset_values: Vec::new(),
    })
}

fn handle_read_offset_all_afe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdReadOffsetAllAfe>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::CmdReadOffsetAllAfeResponse {
                success: false,
                message: format!("Bad cmd_readOffset_allAFE payload: {err}"),
                ..Default::default()
            });
        }
    };

    let command = match afe_id(req.afe_block).map(|afe| AfeCommand::ReadOffset {
        target: ChannelTarget::Afe(afe),
    }) {
        Ok(command) => command,
        Err(err) => {
            return v2::encode(pb::CmdReadOffsetAllAfeResponse {
                success: false,
                message: err,
                afe_block: req.afe_block,
                ..Default::default()
            });
        }
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdReadOffsetAllAfeResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        offset_values: Vec::new(),
    })
}

fn handle_read_vbias_control<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    if let Err(err) = decode::<pb::CmdReadVbiasControl>(payload) {
        return v2::encode(pb::CmdReadVbiasControlResponse {
            success: false,
            message: format!("Bad cmd_readVbiasControl payload: {err}"),
            ..Default::default()
        });
    }

    let outcome = submit(rpu, AfeCommand::ReadVbiasControl);
    v2::encode(pb::CmdReadVbiasControlResponse {
        success: outcome.success,
        message: outcome.message,
        v_bias_control_value: outcome.readback.unwrap_or_default(),
    })
}

fn handle_set_afe_reset<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdSetAfeReset>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::CmdSetAfeResetResponse {
                success: false,
                message: format!("Bad cmd_setAFEReset payload: {err}"),
                ..Default::default()
            });
        }
    };

    let outcome = submit(
        rpu,
        AfeCommand::SetReset {
            asserted: req.reset_value,
        },
    );
    v2::encode(pb::CmdSetAfeResetResponse {
        success: outcome.success,
        message: outcome.message,
        reset_value: req.reset_value,
    })
}

fn handle_do_afe_reset<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    if let Err(err) = decode::<pb::CmdDoAfeReset>(payload) {
        return v2::encode(pb::CmdDoAfeResetResponse {
            success: false,
            message: format!("Bad cmd_doAFEReset payload: {err}"),
        });
    }

    let outcome = submit(rpu, AfeCommand::DoReset);
    v2::encode(pb::CmdDoAfeResetResponse {
        success: outcome.success,
        message: outcome.message,
    })
}

fn handle_set_afe_power_state<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdSetAfePowerState>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::CmdSetAfePowerStateResponse {
                success: false,
                message: format!("Bad cmd_setAFEPowerState payload: {err}"),
                ..Default::default()
            });
        }
    };

    let outcome = submit(
        rpu,
        AfeCommand::SetPowerState {
            enabled: req.power_state,
        },
    );
    v2::encode(pb::CmdSetAfePowerStateResponse {
        success: outcome.success,
        message: outcome.message,
        power_state: outcome.readback.unwrap_or(u32::from(req.power_state)),
    })
}

fn handle_align_afe<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    if let Err(err) = decode::<pb::CmdAlignAfEs>(payload) {
        return v2::encode(pb::CmdAlignAfEsResponse {
            success: false,
            message: format!("Bad cmd_alignAFEs payload: {err}"),
            ..Default::default()
        });
    }

    let outcome = submit(rpu, AfeCommand::Align);
    v2::encode(pb::CmdAlignAfEsResponse {
        success: outcome.success,
        message: outcome.message,
        delay: Vec::new(),
        bitslip: Vec::new(),
    })
}

fn handle_write_afe_function<T: RpuAfeTransport>(payload: &[u8], rpu: &mut T) -> Vec<u8> {
    let req = match decode::<pb::CmdWriteAfeFunction>(payload) {
        Ok(req) => req,
        Err(err) => {
            return v2::encode(pb::CmdWriteAfeFunctionResponse {
                success: false,
                message: format!("Bad cmd_writeAFEFunction payload: {err}"),
                ..Default::default()
            });
        }
    };

    let command = match make_write_function(req.afe_block, req.function.clone(), req.config_value) {
        Ok(command) => command,
        Err(err) => {
            return v2::encode(pb::CmdWriteAfeFunctionResponse {
                success: false,
                message: err,
                afe_block: req.afe_block,
                function: req.function,
                config_value: req.config_value,
            });
        }
    };

    let outcome = submit(rpu, command);
    v2::encode(pb::CmdWriteAfeFunctionResponse {
        success: outcome.success,
        message: outcome.message,
        afe_block: req.afe_block,
        function: req.function,
        config_value: outcome.readback.unwrap_or_default(),
    })
}

struct SubmitOutcome {
    success: bool,
    message: String,
    readback: Option<u32>,
}

fn submit<T: RpuAfeTransport>(rpu: &mut T, command: AfeCommand) -> SubmitOutcome {
    if let Err(err) = command.validate() {
        return SubmitOutcome {
            success: false,
            message: err.to_string(),
            readback: None,
        };
    }

    match rpu.submit_afe_command(command) {
        Ok(reply) => SubmitOutcome {
            success: reply.accepted && reply.applied,
            message: reply.message,
            readback: reply.readback,
        },
        Err(err) => SubmitOutcome {
            success: false,
            message: format!("{RPU_FAIL_PREFIX}{err}"),
            readback: None,
        },
    }
}

fn configure_command(req: pb::ConfigureRequest) -> Result<AfeCommand, String> {
    let mut afes = Vec::with_capacity(req.afes.len());
    for afe in req.afes {
        let adc = afe.adc.unwrap_or_default();
        let pga = afe.pga.unwrap_or_default();
        let lna = afe.lna.unwrap_or_default();
        afes.push(AfeFrontendConfig {
            afe: afe_id(afe.id)?,
            attenuation: u16_value("attenuators", afe.attenuators)?,
            bias: u16_value("v_bias", afe.v_bias)?,
            adc: AdcConfig {
                resolution: adc.resolution,
                output_format: adc.output_format,
                msb_first: adc.sb_first,
            },
            pga: PgaConfig {
                lpf_cut_frequency: u8_value("pga.lpf_cut_frequency", pga.lpf_cut_frequency)?,
                integrator_disable: pga.integrator_disable,
                gain: pga.gain,
            },
            lna: LnaConfig {
                clamp: u8_value("lna.clamp", lna.clamp)?,
                gain: u8_value("lna.gain", lna.gain)?,
                integrator_disable: lna.integrator_disable,
            },
        });
    }

    let mut channels = Vec::with_capacity(req.channels.len());
    for channel in req.channels {
        channels.push(ChannelFrontendConfig {
            channel: channel_id(channel.id)?,
            trim: u16_value("trim", channel.trim)?,
            offset: u16_value("offset", channel.offset)?,
            gain: u16_value("gain", channel.gain)?,
        });
    }

    Ok(AfeCommand::ConfigureFrontend {
        afes,
        channels,
        bias_control: u16_value("biasctrl", req.biasctrl)?,
    })
}

fn make_read_register(afe_block: u32, reg_address: u32) -> Result<AfeCommand, String> {
    Ok(AfeCommand::ReadRegister {
        afe: afe_id(afe_block)?,
        register: u8_value("regAddress", reg_address)?,
    })
}

fn make_write_register(
    afe_block: u32,
    reg_address: u32,
    reg_value: u32,
) -> Result<AfeCommand, String> {
    Ok(AfeCommand::WriteRegister {
        afe: afe_id(afe_block)?,
        register: u8_value("regAddress", reg_address)?,
        value: u16_value("regValue", reg_value)?,
    })
}

fn make_set_attenuation(afe_block: u32, value: u32) -> Result<AfeCommand, String> {
    Ok(AfeCommand::SetAttenuation {
        afe: afe_id(afe_block)?,
        value: u16_value("attenuation", value)?,
    })
}

fn make_set_bias(afe_block: u32, value: u32) -> Result<AfeCommand, String> {
    Ok(AfeCommand::SetBias {
        afe: afe_id(afe_block)?,
        value: u16_value("biasValue", value)?,
    })
}

fn make_set_trim(target: ChannelTarget, value: u32, gain: bool) -> Result<AfeCommand, String> {
    Ok(AfeCommand::SetTrim {
        target,
        value: u16_value("trimValue", value)?,
        gain,
    })
}

fn make_set_offset(target: ChannelTarget, value: u32, gain: bool) -> Result<AfeCommand, String> {
    Ok(AfeCommand::SetOffset {
        target,
        value: u16_value("offsetValue", value)?,
        gain,
    })
}

fn make_set_vbias_control(value: u32, enable: bool) -> Result<AfeCommand, String> {
    Ok(AfeCommand::SetVbiasControl {
        value: u16_value("vBiasControlValue", value)?,
        enable,
    })
}

fn make_write_function(afe_block: u32, name: String, value: u32) -> Result<AfeCommand, String> {
    Ok(AfeCommand::WriteFunction {
        afe: afe_id(afe_block)?,
        name,
        value: u16_value("configValue", value)?,
    })
}

fn afe_id(value: u32) -> Result<AfeId, String> {
    let afe = u8_value("afeBlock", value)?;
    AfeId::from_board(afe).map_err(|err| err.to_string())
}

fn channel_id(value: u32) -> Result<ChannelId, String> {
    let channel = u8_value("channel", value)?;
    ChannelId::new(channel).map_err(|err| err.to_string())
}

fn u8_value(name: &str, value: u32) -> Result<u8, String> {
    value
        .try_into()
        .map_err(|_| format!("{name} out of range for u8: {value}"))
}

fn u16_value(name: &str, value: u32) -> Result<u16, String> {
    value
        .try_into()
        .map_err(|_| format!("{name} out of range for u16: {value}"))
}

fn decode<M: Message + Default>(payload: &[u8]) -> Result<M, prost::DecodeError> {
    M::decode(payload)
}
