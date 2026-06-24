# RPU AFE Boundary

AFE control is treated as RPU-owned. The Linux Rust server must not perform
direct AFE writes through `/dev/mem` as a fallback path.

## Linux Responsibilities

- Accept existing protobuf/ZMQ requests.
- Preserve compatibility with the current `ControlEnvelopeV2` framing.
- Validate obvious API ranges before crossing into the RPU path.
- Enforce the Linux-visible preflight contract before any AFE request reaches
  the RPU transport.
- Aggregate slow-control status from Linux-visible sysfs/I2C/SPI sources.
- Report RPU availability, firmware name, heartbeat, and last fault.
- Fail AFE-affecting requests closed if the RPU backend is unavailable.

## RPU Responsibilities

- Sequence AFE register writes.
- Sequence AFE DAC gain, bias, trim, offset, and VBIAS writes.
- Own AFE reset and power-state transitions.
- Own the safety/interlock loop for AFE-related hardware state.
- Maintain the authoritative cached AFE state used by readback requests.
- Expose heartbeat, fault state, and command result state to Linux.

## Commands Routed To RPU

- `MT2_CONFIGURE_FE_REQ`
- `MT2_WRITE_AFE_REG_REQ`
- `MT2_WRITE_AFE_VGAIN_REQ`
- `MT2_WRITE_AFE_BIAS_SET_REQ`
- `MT2_WRITE_AFE_ATTENUATION_REQ`
- `MT2_WRITE_TRIM_ALL_CH_REQ`
- `MT2_WRITE_TRIM_ALL_AFE_REQ`
- `MT2_WRITE_TRIM_CH_REQ`
- `MT2_WRITE_OFFSET_ALL_CH_REQ`
- `MT2_WRITE_OFFSET_ALL_AFE_REQ`
- `MT2_WRITE_OFFSET_CH_REQ`
- `MT2_WRITE_VBIAS_CONTROL_REQ`
- `MT2_READ_AFE_REG_REQ`
- `MT2_READ_AFE_VGAIN_REQ`
- `MT2_READ_AFE_BIAS_SET_REQ`
- `MT2_READ_TRIM_ALL_CH_REQ`
- `MT2_READ_TRIM_ALL_AFE_REQ`
- `MT2_READ_TRIM_CH_REQ`
- `MT2_READ_OFFSET_ALL_CH_REQ`
- `MT2_READ_OFFSET_ALL_AFE_REQ`
- `MT2_READ_OFFSET_CH_REQ`
- `MT2_READ_VBIAS_CONTROL_REQ`
- `MT2_SET_AFE_RESET_REQ`
- `MT2_DO_AFE_RESET_REQ`
- `MT2_SET_AFE_POWERSTATE_REQ`
- `MT2_ALIGN_AFE_REQ`
- `MT2_WRITE_AFE_FUNCTION_REQ`

## Commands Remaining Linux-Side Initially

- Clock-chip service status, clock reachability, and endpoint/timing status.
- Spybuffer dumps.
- Software trigger.
- Trigger counters.
- General slow-control monitoring.
- Firmware, FPGA manager, service, and remoteproc status.

Some of these may later move if they become part of the RPU safety loop.

## Preflight Gate

All commands routed to the RPU must pass the hardware preflight checks described
in `docs/preflight-contract.md`. This ensures AFE configuration cannot happen
before the clock chip is configured and reachable, the PL is loaded, the timing
endpoint service is healthy, and Linux-visible I2C/SPI/RPU interfaces exist.

## First RPU Transport

The first implementation should use the existing Linux `remoteproc` bring-up and
add an RPMsg or shared-memory command queue. The command protocol should be
fixed-size and bounded; protobuf is kept on the external ZMQ API, not required
inside the real-time core.

The first fixed-size wire ABI and optional Linux RPMsg/file transport are
defined in `docs/rpu-wire-abi.md`. Until an RPU firmware endpoint exists, the
server still defaults to the fail-closed transport.

Minimum shared status:

- ABI version.
- RPU firmware build ID.
- Heartbeat counter.
- Last accepted command sequence.
- Last applied command sequence.
- Last fault code.
- Interlock state.
- AFE reset/power state.
