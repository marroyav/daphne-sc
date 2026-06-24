use daphne_sc_core::afe::AfeCommand;
use daphne_sc_core::rpu::{AfeCommandReply, RpuAfeTransport, RpuError, RpuLinkStatus};
use daphne_sc_core::rpu_wire::{RpuWireCommand, RpuWireReply, RpuWireStatus, RPU_WIRE_REPLY_LEN};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

pub struct RpmsgAfeTransport {
    file: File,
    next_sequence: u64,
    timeout: Duration,
    last_heartbeat: Option<u64>,
    last_fault: Option<String>,
}

impl RpmsgAfeTransport {
    pub fn open(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, RpuError> {
        let path_ref = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path_ref)
            .map_err(|err| RpuError::Transport(format!("opening {}: {err}", path_ref.display())))?;

        Ok(Self {
            file,
            next_sequence: 1,
            timeout,
            last_heartbeat: None,
            last_fault: None,
        })
    }

    fn transact(&mut self, command: RpuWireCommand) -> Result<RpuWireReply, RpuError> {
        self.file
            .write_all(&command.encode())
            .map_err(|err| RpuError::Transport(format!("writing RPU command: {err}")))?;
        self.file
            .flush()
            .map_err(|err| RpuError::Transport(format!("flushing RPU command: {err}")))?;

        wait_readable(self.file.as_raw_fd(), self.timeout)?;

        let mut reply = [0_u8; RPU_WIRE_REPLY_LEN];
        self.file
            .read_exact(&mut reply)
            .map_err(|err| RpuError::Transport(format!("reading RPU reply: {err}")))?;
        let reply = RpuWireReply::decode(&reply)
            .map_err(|err| RpuError::Transport(format!("decoding RPU reply: {err}")))?;
        if reply.sequence != command.sequence {
            return Err(RpuError::Transport(format!(
                "RPU reply sequence mismatch: sent {}, got {}",
                command.sequence, reply.sequence
            )));
        }
        self.last_heartbeat = Some(reply.heartbeat);
        if reply.fault_code != 0 {
            self.last_fault = Some(format!("RPU fault code {}", reply.fault_code));
        }
        Ok(reply)
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        sequence
    }
}

impl RpuAfeTransport for RpmsgAfeTransport {
    fn status(&mut self) -> Result<RpuLinkStatus, RpuError> {
        let sequence = self.next_sequence();
        let status_command = RpuWireCommand::status(sequence);
        match self.transact(status_command) {
            Ok(reply) => Ok(RpuLinkStatus {
                available: true,
                running: !matches!(reply.status, RpuWireStatus::Fault | RpuWireStatus::Timeout),
                firmware: Some("rpmsg-rpu-wire-abi-v2".to_string()),
                heartbeat: Some(reply.heartbeat),
                last_fault: self.last_fault.clone(),
            }),
            Err(err) => Err(err),
        }
    }

    fn submit_afe_command(&mut self, command: AfeCommand) -> Result<AfeCommandReply, RpuError> {
        command
            .validate()
            .map_err(|err| RpuError::Rejected(err.to_string()))?;
        let frame_count = RpuWireCommand::frame_count_for_afe_command(&command)
            .map_err(|err| RpuError::Rejected(err.to_string()))?;
        let first_sequence = self.reserve_sequences(frame_count);
        let frames = RpuWireCommand::from_afe_command_sequence(first_sequence, &command)
            .map_err(|err| RpuError::Rejected(err.to_string()))?;

        let mut final_reply = None;
        for frame in frames {
            let reply = self.transact(frame)?;
            final_reply = Some(reply);
        }
        let reply = final_reply
            .ok_or_else(|| RpuError::Transport("RPU command produced no frames".to_string()))?;

        let (accepted, applied, message) = match reply.status {
            RpuWireStatus::Accepted => (true, false, "RPU accepted command".to_string()),
            RpuWireStatus::Applied => (true, true, "RPU applied command".to_string()),
            RpuWireStatus::Rejected => (false, false, "RPU rejected command".to_string()),
            RpuWireStatus::Interlocked => (
                false,
                false,
                format!("RPU interlock active: code {}", reply.interlock_code),
            ),
            RpuWireStatus::Fault => (
                false,
                false,
                format!("RPU fault active: code {}", reply.fault_code),
            ),
            RpuWireStatus::Timeout => (true, false, "RPU command timed out".to_string()),
        };

        Ok(AfeCommandReply {
            accepted,
            applied,
            readback: reply.readback,
            message,
        })
    }
}

impl RpmsgAfeTransport {
    fn reserve_sequences(&mut self, count: usize) -> u64 {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        if count == 0 {
            return self.next_sequence;
        }
        if self.next_sequence > u64::MAX.saturating_sub(count) {
            self.next_sequence = 1;
        }

        let first = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(count).max(1);
        first
    }
}

fn wait_readable(fd: i32, timeout: Duration) -> Result<(), RpuError> {
    let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    // SAFETY: poll is called with a valid pointer to one pollfd and a finite
    // timeout. The file descriptor is owned by RpmsgAfeTransport.
    let rc = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if rc < 0 {
        return Err(RpuError::Transport(format!(
            "polling RPU transport: {}",
            std::io::Error::last_os_error()
        )));
    }
    if rc == 0 {
        return Err(RpuError::Timeout);
    }
    if pollfd.revents & libc::POLLIN == 0 {
        return Err(RpuError::Transport(format!(
            "RPU transport not readable after poll: revents=0x{:X}",
            pollfd.revents
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn wait_readable_times_out_on_empty_socket() {
        let (host, _rpu) = UnixStream::pair().unwrap();

        assert!(matches!(
            wait_readable(host.as_raw_fd(), Duration::from_millis(1)),
            Err(RpuError::Timeout)
        ));
    }

    #[test]
    fn wait_readable_accepts_socket_data() {
        let (host, mut rpu) = UnixStream::pair().unwrap();
        rpu.write_all(&[1]).unwrap();

        wait_readable(host.as_raw_fd(), Duration::from_millis(100)).unwrap();
    }
}
