# Hardware Preflight Contract

The Rust server must fail closed before any hardware-affecting AFE request is
sent to the RPU transport unless the board-level timing and PL prerequisites
are healthy.

This preserves the current `daphne-server` and `daphne-firmware` bring-up
contract while the AFE slow-control path is moved toward the Kria real-time
cores.

## Required State Before AFE Commands

AFE commands include frontend configuration, AFE register reads/writes, DAC
gain/bias/trim/offset writes, VBIAS writes, reset, power-state transitions, AFE
alignment, and AFE function writes.

Before those commands can cross into the RPU path, Linux must report:

- `firmware.service` is active.
- `clockchip.service` is active.
- `endpoint.service` is active.
- `/sys/class/fpga_manager/fpga0/state` is `operating`.
- The PL I2C and SPI platform devices are present.
- The Linux I2C and SPI device nodes are present.
- The configured clock chip address is reachable over I2C.
- Linux can see the remoteproc interface for the R5/RPU cores.

If any check fails, the protobuf response keeps the normal response type but
returns `success=false` with a `preflight failed: ...` message listing the
failed checks.

## Clock-Chip Ownership

The live DAPHNE-15 service currently owns clock-chip programming through
`clockchip.service` and `/usr/local/bin/daphne-clockchip.sh`. The Rust server
does not silently reprogram the chip on every request.

The initial Rust integration treats the clock service as the authoritative
board-level prerequisite and validates that the configured chip is reachable.
When the clock owner is moved into the Rust/RPU stack, the register table and
verification logic from the clock-chip service should be ported explicitly and
kept separate from endpoint MMCM and timestamp readiness checks.

## Endpoint Readiness Is Separate

Clock-chip configuration, FPGA manager `operating`, endpoint MMCM lock, endpoint
FSM state, and timestamp validity are different signals. A future status API
should expose those raw checks as separate fields as well as a derived
`timing_usable` summary.

The AFE path must never infer that selecting a clock source means timing is
ready.
