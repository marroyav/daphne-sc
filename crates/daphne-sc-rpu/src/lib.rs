#![no_std]

use daphne_sc_core::{
    RpuWireCommand, RpuWireError, RpuWireOp, RpuWireReply, RpuWireStatus, RpuWireTarget,
    RPU_WIRE_ABI_VERSION, RPU_WIRE_COMMAND_LEN, RPU_WIRE_REPLY_LEN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpuConfig {
    pub abi_version: u16,
}

impl Default for RpuConfig {
    fn default() -> Self {
        Self {
            abi_version: RPU_WIRE_ABI_VERSION,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InterlockState {
    pub clock_ready: bool,
    pub firmware_ready: bool,
    pub interfaces_ready: bool,
    pub temperatures_ok: bool,
    pub rails_ok: bool,
    pub host_heartbeat_ok: bool,
    pub fault_code: u32,
}

impl InterlockState {
    pub fn ready_for_afe(self) -> bool {
        self.clock_ready
            && self.firmware_ready
            && self.interfaces_ready
            && self.temperatures_ok
            && self.rails_ok
            && self.host_heartbeat_ok
            && self.fault_code == 0
    }

    pub fn code(self) -> u32 {
        let mut code = 0_u32;
        code |= u32::from(!self.clock_ready);
        code |= u32::from(!self.firmware_ready) << 1;
        code |= u32::from(!self.interfaces_ready) << 2;
        code |= u32::from(!self.temperatures_ok) << 3;
        code |= u32::from(!self.rails_ok) << 4;
        code |= u32::from(!self.host_heartbeat_ok) << 5;
        code |= u32::from(self.fault_code != 0) << 6;
        code
    }
}

pub trait AfeHardware {
    type Error;

    fn read_register(&mut self, afe_pl: u8, register: u16) -> Result<u32, Self::Error>;
    fn write_register(&mut self, afe_pl: u8, register: u16, value: u32)
        -> Result<u32, Self::Error>;
    fn read_scalar(&mut self, op: RpuWireOp, afe_pl: u8) -> Result<u32, Self::Error>;
    fn write_scalar(&mut self, op: RpuWireOp, afe_pl: u8, value: u32) -> Result<u32, Self::Error>;
    fn read_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error>;
    fn write_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error>;
    fn read_vbias_control(&mut self) -> Result<u32, Self::Error>;
    fn write_vbias_control(&mut self, value: u32, enable: bool) -> Result<u32, Self::Error>;
    fn set_reset(&mut self, asserted: bool) -> Result<(), Self::Error>;
    fn do_reset(&mut self) -> Result<(), Self::Error>;
    fn set_power_state(&mut self, enabled: bool) -> Result<(), Self::Error>;
    fn align(&mut self) -> Result<(), Self::Error>;
}

pub struct RpuRuntime<H> {
    hardware: H,
    heartbeat: u64,
    interlock: InterlockState,
}

impl<H> RpuRuntime<H> {
    pub fn new(hardware: H) -> Self {
        Self {
            hardware,
            heartbeat: 0,
            interlock: InterlockState::default(),
        }
    }

    pub fn interlock(&self) -> InterlockState {
        self.interlock
    }

    pub fn set_interlock(&mut self, interlock: InterlockState) {
        self.interlock = interlock;
    }

    pub fn hardware_mut(&mut self) -> &mut H {
        &mut self.hardware
    }
}

impl<H: AfeHardware> RpuRuntime<H> {
    pub fn handle_frame(&mut self, input: &[u8; RPU_WIRE_COMMAND_LEN]) -> [u8; RPU_WIRE_REPLY_LEN] {
        self.heartbeat = self.heartbeat.wrapping_add(1);
        match RpuWireCommand::decode(input) {
            Ok(command) => self.handle_command(command).encode(),
            Err(err) => self.error_reply(0, err).encode(),
        }
    }

    fn handle_command(&mut self, command: RpuWireCommand) -> RpuWireReply {
        if command.op == RpuWireOp::Status {
            return RpuWireReply {
                sequence: command.sequence,
                status: if self.interlock.fault_code == 0 {
                    RpuWireStatus::Applied
                } else {
                    RpuWireStatus::Fault
                },
                readback: Some(self.interlock.code()),
                heartbeat: self.heartbeat,
                fault_code: self.interlock.fault_code,
                interlock_code: self.interlock.code(),
            };
        }

        if !self.interlock.ready_for_afe() {
            return RpuWireReply {
                sequence: command.sequence,
                status: RpuWireStatus::Interlocked,
                readback: None,
                heartbeat: self.heartbeat,
                fault_code: self.interlock.fault_code,
                interlock_code: self.interlock.code(),
            };
        }

        match self.apply_command(&command) {
            Ok(readback) => RpuWireReply {
                sequence: command.sequence,
                status: RpuWireStatus::Applied,
                readback,
                heartbeat: self.heartbeat,
                fault_code: 0,
                interlock_code: 0,
            },
            Err(ApplyError::Rejected) => RpuWireReply {
                sequence: command.sequence,
                status: RpuWireStatus::Rejected,
                readback: None,
                heartbeat: self.heartbeat,
                fault_code: 0,
                interlock_code: 0,
            },
            Err(ApplyError::HardwareFault) => RpuWireReply {
                sequence: command.sequence,
                status: RpuWireStatus::Fault,
                readback: None,
                heartbeat: self.heartbeat,
                fault_code: 1,
                interlock_code: self.interlock.code(),
            },
        }
    }

    fn apply_command(&mut self, command: &RpuWireCommand) -> Result<Option<u32>, ApplyError> {
        use RpuWireOp::*;

        match command.op {
            Status => Ok(Some(self.interlock.code())),
            ReadRegister => {
                let afe = require_afe(command)?;
                hw(self.hardware.read_register(afe, command.register)).map(Some)
            }
            WriteRegister => {
                let afe = require_afe(command)?;
                hw(self
                    .hardware
                    .write_register(afe, command.register, command.value))
                .map(Some)
            }
            ReadAttenuation | ReadBias => {
                let afe = require_afe(command)?;
                hw(self.hardware.read_scalar(command.op, afe)).map(Some)
            }
            SetAttenuation | SetBias => {
                let afe = require_afe(command)?;
                hw(self.hardware.write_scalar(command.op, afe, command.value)).map(Some)
            }
            ReadTrim | ReadOffset => hw(self.hardware.read_channel_scalar(command)).map(Some),
            SetTrim | SetOffset => hw(self.hardware.write_channel_scalar(command)).map(Some),
            ReadVbiasControl => hw(self.hardware.read_vbias_control()).map(Some),
            SetVbiasControl => hw(self
                .hardware
                .write_vbias_control(command.value, command.flags != 0))
            .map(Some),
            SetReset => hw(self.hardware.set_reset(command.flags != 0)).map(|()| None),
            DoReset => hw(self.hardware.do_reset()).map(|()| None),
            SetPowerState => hw(self.hardware.set_power_state(command.flags != 0)).map(|()| None),
            Align => hw(self.hardware.align()).map(|()| None),
        }
    }

    fn error_reply(&self, sequence: u64, _err: RpuWireError) -> RpuWireReply {
        RpuWireReply {
            sequence,
            status: RpuWireStatus::Rejected,
            readback: None,
            heartbeat: self.heartbeat,
            fault_code: 0,
            interlock_code: self.interlock.code(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplyError {
    Rejected,
    HardwareFault,
}

fn require_afe(command: &RpuWireCommand) -> Result<u8, ApplyError> {
    if command.target == RpuWireTarget::Afe {
        Ok(command.afe_pl)
    } else {
        Err(ApplyError::Rejected)
    }
}

fn hw<T, E>(result: Result<T, E>) -> Result<T, ApplyError> {
    result.map_err(|_| ApplyError::HardwareFault)
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use daphne_sc_core::{RpuWireCommand, RpuWireStatus};

    #[derive(Default)]
    struct FakeHardware {
        writes: std::vec::Vec<(u8, u16, u32)>,
        reset: bool,
    }

    impl AfeHardware for FakeHardware {
        type Error = ();

        fn read_register(&mut self, afe_pl: u8, register: u16) -> Result<u32, Self::Error> {
            Ok((u32::from(afe_pl) << 16) | u32::from(register))
        }

        fn write_register(
            &mut self,
            afe_pl: u8,
            register: u16,
            value: u32,
        ) -> Result<u32, Self::Error> {
            self.writes.push((afe_pl, register, value));
            Ok(value)
        }

        fn read_scalar(&mut self, _op: RpuWireOp, afe_pl: u8) -> Result<u32, Self::Error> {
            Ok(u32::from(afe_pl))
        }

        fn write_scalar(
            &mut self,
            _op: RpuWireOp,
            _afe_pl: u8,
            value: u32,
        ) -> Result<u32, Self::Error> {
            Ok(value)
        }

        fn read_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error> {
            Ok(u32::from(command.channel))
        }

        fn write_channel_scalar(&mut self, command: &RpuWireCommand) -> Result<u32, Self::Error> {
            Ok(command.value)
        }

        fn read_vbias_control(&mut self) -> Result<u32, Self::Error> {
            Ok(0)
        }

        fn write_vbias_control(&mut self, value: u32, _enable: bool) -> Result<u32, Self::Error> {
            Ok(value)
        }

        fn set_reset(&mut self, asserted: bool) -> Result<(), Self::Error> {
            self.reset = asserted;
            Ok(())
        }

        fn do_reset(&mut self) -> Result<(), Self::Error> {
            self.reset = true;
            self.reset = false;
            Ok(())
        }

        fn set_power_state(&mut self, _enabled: bool) -> Result<(), Self::Error> {
            Ok(())
        }

        fn align(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn ready() -> InterlockState {
        InterlockState {
            clock_ready: true,
            firmware_ready: true,
            interfaces_ready: true,
            temperatures_ok: true,
            rails_ok: true,
            host_heartbeat_ok: true,
            fault_code: 0,
        }
    }

    #[test]
    fn status_reports_interlock_code() {
        let mut runtime = RpuRuntime::new(FakeHardware::default());
        let frame = RpuWireCommand::status(5).encode();

        let reply = RpuWireReply::decode(&runtime.handle_frame(&frame)).unwrap();

        assert_eq!(reply.sequence, 5);
        assert_eq!(reply.status, RpuWireStatus::Applied);
        assert_ne!(reply.readback, Some(0));
    }

    #[test]
    fn interlock_blocks_afe_commands() {
        let mut runtime = RpuRuntime::new(FakeHardware::default());
        let command = RpuWireCommand {
            sequence: 9,
            op: RpuWireOp::WriteRegister,
            target: RpuWireTarget::Afe,
            afe_board: 1,
            afe_pl: 4,
            channel: 0,
            afe_channel: 0,
            register: 3,
            value: 0x1234,
            flags: 0,
        };

        let reply = RpuWireReply::decode(&runtime.handle_frame(&command.encode())).unwrap();

        assert_eq!(reply.status, RpuWireStatus::Interlocked);
        assert_eq!(runtime.hardware_mut().writes.len(), 0);
    }

    #[test]
    fn ready_runtime_applies_register_write() {
        let mut runtime = RpuRuntime::new(FakeHardware::default());
        runtime.set_interlock(ready());
        let command = RpuWireCommand {
            sequence: 10,
            op: RpuWireOp::WriteRegister,
            target: RpuWireTarget::Afe,
            afe_board: 1,
            afe_pl: 4,
            channel: 0,
            afe_channel: 0,
            register: 3,
            value: 0x1234,
            flags: 0,
        };

        let reply = RpuWireReply::decode(&runtime.handle_frame(&command.encode())).unwrap();

        assert_eq!(reply.status, RpuWireStatus::Applied);
        assert_eq!(reply.readback, Some(0x1234));
        assert_eq!(runtime.hardware_mut().writes, [(4, 3, 0x1234)]);
    }
}
