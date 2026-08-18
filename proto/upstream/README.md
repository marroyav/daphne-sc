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

The telemetry schema is intentionally generic. A new DAPHNE variable is a new
typed `TelemetryPoint`/registry NodeId and normally does not require another
protobuf message or field.
