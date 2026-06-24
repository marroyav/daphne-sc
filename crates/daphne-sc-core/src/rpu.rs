use crate::afe::AfeCommand;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfeCommandReply {
    pub accepted: bool,
    pub applied: bool,
    pub readback: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpuLinkStatus {
    pub available: bool,
    pub running: bool,
    pub firmware: Option<String>,
    pub heartbeat: Option<u64>,
    pub last_fault: Option<String>,
}

pub trait RpuAfeTransport {
    fn status(&mut self) -> Result<RpuLinkStatus, RpuError>;
    fn submit_afe_command(&mut self, command: AfeCommand) -> Result<AfeCommandReply, RpuError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpuError {
    Unavailable,
    Transport(String),
    Rejected(String),
    Timeout,
}

impl fmt::Display for RpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "RPU AFE backend is unavailable"),
            Self::Transport(msg) => write!(f, "RPU transport error: {msg}"),
            Self::Rejected(msg) => write!(f, "RPU rejected command: {msg}"),
            Self::Timeout => write!(f, "RPU command timed out"),
        }
    }
}

impl std::error::Error for RpuError {}

#[derive(Debug, Default)]
pub struct FailClosedRpuTransport;

impl RpuAfeTransport for FailClosedRpuTransport {
    fn status(&mut self) -> Result<RpuLinkStatus, RpuError> {
        Ok(RpuLinkStatus {
            available: false,
            running: false,
            firmware: None,
            heartbeat: None,
            last_fault: Some("RPU AFE transport is not configured".to_string()),
        })
    }

    fn submit_afe_command(&mut self, _command: AfeCommand) -> Result<AfeCommandReply, RpuError> {
        Err(RpuError::Unavailable)
    }
}
