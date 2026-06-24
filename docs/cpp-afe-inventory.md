# C++ AFE Inventory

This is the first parity list for moving `daphneServer` AFE behavior into the
Rust/RPU split.

## Current Sources

- `daphneZMQ/srcs/server_controller/handlers.cpp`
- `daphneZMQ/srcs/Afe.cpp`
- `daphneZMQ/srcs/Dac.cpp`
- `daphneZMQ/srcs/defines.hpp`
- `daphneZMQ/srcs/FpgaRegDict.cpp`

## Board Mapping

The current C++ server maps user-facing board AFE numbers to PL AFE numbers as:

| Board AFE | PL AFE |
| --- | --- |
| 0 | 0 |
| 1 | 4 |
| 2 | 3 |
| 3 | 2 |
| 4 | 1 |

The Rust core keeps this in `AFE_BOARD_TO_PL`.

## Direct AFE Register Path

Current C++ behavior:

1. Validate the register is in the AFE register list.
2. Write `(register & 0xff) << 16 | (value & 0xffff)` to `afeControl_N`.
3. Toggle command values through the FPGA register path.
4. Read back lower 16 bits.
5. Clear the command register.

Rust/RPU target behavior:

- Linux validates envelope/payload shape and basic ranges.
- RPU performs the register sequence and reports readback.
- RPU owns authoritative cached register state.

## DAC-Backed AFE Commands

These commands currently write via FPGA DAC registers and update C++ cached state:

- attenuation/gain
- bias
- trim
- offset
- VBIAS control
- bias enable

Rust/RPU target behavior:

- RPU performs DAC sequencing and busy waits.
- RPU maintains trim/offset/bias/attenuation cache.
- Linux readback requests query RPU state, not a separate Linux cache.

## Reset And Power

Current C++ behavior writes `afeGlobalControl.RESET` and
`afeGlobalControl.POWERSTATE`.

Rust/RPU target behavior:

- RPU owns reset and power transitions.
- RPU interlock policy decides whether a requested transition is allowed.
- Linux reports rejected transitions as protobuf failures.

## Alignment

`ALIGN_AFE` currently touches frontend delay/bitslip and spybuffer frame-clock
inspection. It is classified as RPU-routed for now because it is AFE stateful and
changes frontend capture behavior.

