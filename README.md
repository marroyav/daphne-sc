# daphne-sc

Rust slow-control work area for DAPHNE/Kria.

The intended split is:

- Linux APU: protobuf/ZMQ API, status aggregation, logging, deployment integration.
- RPU: AFE configuration, AFE DAC writes, AFE reset/power sequencing, and safety/interlock logic.

The first implementation target is a Linux Rust server that is protocol-compatible with
`daphneServer` but routes all AFE-affecting commands through an explicit RPU interface.
Until the RPU transport is implemented, those commands must fail closed instead of falling
back to Linux-side `/dev/mem` writes.

AFE commands also pass a Linux-visible hardware preflight gate before reaching the RPU
transport. The gate requires the firmware, clockchip, and endpoint service chain to be
healthy, the FPGA manager to report `operating`, and the I2C/SPI/RPU interfaces to be
present and reachable.

Current Rust migration pieces:

- `clockchip_tool` verifies/programs the external clock chip through `/dev/i2c-*`.
- `SlowControlStatusRequest` uses message type `1000` and reports preflight/RPU status.
- `daphne-sc-server --rpu-rpmsg PATH` selects the fixed-size RPU wire transport.

## Current Starting Point

- DAPHNE-15 has Linux `remoteproc` entries for both R5 cores.
- DAPHNE-15 exposes `/dev/i2c-1`, `/dev/i2c-2`, and `/dev/spidev3.0`.
- The current C++ server listens on `tcp://*:40001`.
- The Rust server should initially run beside it on `tcp://*:40002`.

## Layout

- `crates/daphne-sc-core`: shared command/status model and routing policy.
- `crates/daphne-sc-server`: Linux userspace server entry point.
- `proto`: additive protobuf messages for status and RPU-backed AFE control.
- `docs`: migration notes and hardware ownership decisions.
