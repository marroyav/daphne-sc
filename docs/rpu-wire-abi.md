# RPU Wire ABI

The external slow-control API remains protobuf/ZMQ. The real-time core should
not parse protobuf.

The Linux server now has a fixed-size little-endian RPU command/reply ABI for
AFE operations. It is implemented in `daphne-sc-core/src/rpu_wire.rs` and used
by the optional Linux RPMsg transport in `daphne-sc-server/src/rpmsg.rs`.

## Command Transport

The Linux server can be started with an RPMsg-like device:

```bash
daphne-sc-server --rpu-rpmsg /dev/rpmsg_daphne_afe --rpu-timeout-ms 200 tcp://*:40002
```

If `--rpu-rpmsg` is not provided, AFE commands continue to fail closed.

## Fixed-Size Frames

- Command frame size: 64 bytes.
- Reply frame size: 64 bytes.
- ABI version: 2.
- Endianness: little-endian.
- Magic: `0x52505344`.

The current frame supports bounded scalar/register/reset/power/align commands:

- read/write AFE register;
- read/set attenuation;
- read/set bias;
- read/set trim;
- read/set offset;
- read/set VBIAS control;
- set/do AFE reset;
- set AFE power state;
- align AFE;
- status/heartbeat query.
- write AFE function with a bounded UTF-8 function name.
- configure frontend through a staged multi-frame sequence.

## Frontend Configuration Sequence

`ConfigureFrontend` is sent as multiple 64-byte command frames:

1. `BeginConfigureFrontend`: carries `bias_control`, expected AFE record count,
   and expected channel record count.
2. One `ConfigureAfe` frame per AFE record: carries board/PL AFE ID,
   attenuation, bias, ADC flags, PGA fields, and LNA fields.
3. One `ConfigureChannel` frame per channel record: carries channel ID,
   board/PL AFE ID, trim, offset, and gain.
4. `ApplyConfigureFrontend`: tells the RPU to apply the staged configuration.

The RPU runtime rejects `ApplyConfigureFrontend` unless the begin frame was
received and the expected number of AFE and channel records has been staged.
The hardware implementation behind the RPU trait should treat per-record calls
as staging operations and make hardware-visible changes only on apply.

`WriteFunction` is a single frame. The function name is limited to the 31-byte
command payload, with the first payload byte carrying the name length.

## Interlock Semantics

The RPU reply status distinguishes:

- `Applied`: command completed and hardware/cache state was updated.
- `Rejected`: command was invalid for the current RPU state.
- `Interlocked`: safety interlock blocked the command.
- `Fault`: RPU safety loop is in fault state.
- `Timeout`: RPU accepted but did not complete in time.

Linux maps these statuses back into the existing protobuf response type with
`success=false` unless the reply is `Applied`.
