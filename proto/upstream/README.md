# Mirrored DAPHNE protobuf schemas

These files are vendored build inputs so `daphne-sc` can be built and deployed
without a sibling checkout. They are not the canonical place to design the
protocol.

Canonical sources:

```text
daphneZMQ/srcs/protobuf/daphne_v8_telemetry.proto
daphneZMQ/srcs/protobuf/daphneV3_high_level_confs.proto
daphneZMQ/srcs/protobuf/daphneV3_low_level_confs.proto
```

Update the bridge by copying reviewed canonical files here and then comparing
their SHA-256 hashes. Do not independently edit both copies: a byte-for-byte
match makes the deployed wire contract traceable.

The telemetry schema is deliberately explicit. `BoardTelemetry` contains one
named, typed, stable-numbered field for every board-owned variable pattern.
Indexed hardware families use typed repeated wrappers whose instance keys are
also declared in Protobuf. Field annotations carry the OPC-UA NodeId pattern,
engineering unit, data source, and control owner.

A new DAPHNE wire variable therefore requires a reviewed new field in the
canonical `.proto`; it must never be invented in this bridge. Generate the
canonical schema and its one-row-per-field trace ledger from Interface2, then
copy the reviewed schema here byte-for-byte. Released field numbers are never
renumbered or reused.

`daphne_v8_telemetry.proto` defines the read-snapshot request and response.
DAQ configuration requests remain in `daphneV3_high_level_confs.proto` and the
DAQ framework; this bridge does not become their owner.
