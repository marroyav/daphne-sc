#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod afe;
pub mod clockchip;
pub mod rpu;
pub mod rpu_wire;
pub mod status;
pub mod transport;

pub use afe::{AfeCommand, AfeId, ChannelId};
pub use clockchip::{
    program_clock_chip, verify_clock_chip, ClockChipBus, ClockChipError, ClockChipProgramOptions,
    ClockChipRange, ClockChipRegister, CLOCKCHIP_DEFAULT_ADDR, CLOCKCHIP_DISCOVERY_ADDRS,
    CLOCKCHIP_REGISTERS,
};
pub use rpu::{AfeCommandReply, RpuAfeTransport, RpuError, RpuLinkStatus};
pub use rpu_wire::{
    RpuWireAfeConfig, RpuWireChannelConfig, RpuWireCommand, RpuWireConfigCounts, RpuWireError,
    RpuWireOp, RpuWireReply, RpuWireStatus, RpuWireTarget, RPU_WIRE_ABI_VERSION,
    RPU_WIRE_COMMAND_LEN, RPU_WIRE_FUNCTION_NAME_MAX, RPU_WIRE_MAGIC, RPU_WIRE_PAYLOAD_LEN,
    RPU_WIRE_REPLY_LEN,
};
pub use status::SlowControlStatus;
pub use transport::{route_message_type, CommandRoute, MessageTypeV2};
