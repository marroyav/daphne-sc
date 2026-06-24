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
- ABI version: 1.
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

`ConfigureFrontend` and `WriteFunction` are intentionally rejected until the
chunked/string-dictionary extension is added. They must not be silently
translated into partial command sequences on Linux.

## Interlock Semantics

The RPU reply status distinguishes:

- `Applied`: command completed and hardware/cache state was updated.
- `Rejected`: command was invalid for the current RPU state.
- `Interlocked`: safety interlock blocked the command.
- `Fault`: RPU safety loop is in fault state.
- `Timeout`: RPU accepted but did not complete in time.

Linux maps these statuses back into the existing protobuf response type with
`success=false` unless the reply is `Applied`.
