# Migration Plan

## Milestone 1: Rust Compatibility Shell

- Create Rust workspace.
- Keep existing protobuf/ZMQ envelope compatibility.
- Route AFE-affecting commands to an explicit RPU backend.
- Fail AFE-affecting commands closed while RPU transport is missing.
- Add read-only slow-control status API.

## Milestone 2: Linux Status Collectors

- Firmware loaded status:
  - FPGA manager state.
  - overlay/build metadata.
  - expected PL device presence.
- RPU status:
  - `/sys/class/remoteproc/remoteproc*/state`
  - firmware names
  - heartbeat once RPU firmware provides one
- I2C status:
  - expected devices on `/dev/i2c-1` and `/dev/i2c-2`
  - PMBus health for board rails
  - clock chip and expanders
- Clock status:
  - endpoint clock status register
  - MMCM lock bits
- Temperatures:
  - Linux thermal zones
  - PMBus temperatures where available

## Milestone 3: RPU Link

- Add RPMsg or shared-memory transport.
- Add bounded command queue.
- Add heartbeat and fault status.
- Load RPU firmware through Linux `remoteproc`.
- Keep all AFE writes disabled unless the RPU firmware version matches the
  expected ABI.

## Milestone 4: AFE Command Parity

- Implement direct AFE register writes.
- Implement attenuation, bias, trim, offset, VBIAS control.
- Implement reset and power-state transitions.
- Implement configure-FE sequencing.
- Implement AFE readback from RPU-owned cache/readback.
- Compare every response against the C++ server on DAPHNE-15.

## Milestone 5: Interlock Ownership

- Move the safety-critical monitoring loop into the RPU firmware.
- Define fail-safe behavior for loss of heartbeat or command timeout.
- Ensure Linux server death cannot leave unsafe AFE state transitions in flight.

