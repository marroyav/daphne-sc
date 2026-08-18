# V8 DAPHNE to OPC-UA code path: start here

The native read-only path is deliberately limited to four steps:

```text
DaphneClient::Poll()
  -> v8::MakeRequest() + ControlEnvelopeV2/ZMQ
  -> v8::ParseAndValidate()
  -> OpcUaSnapshotWriter::Publish()
  -> OPC-UA variables
```

| Step | File and symbol | Responsibility |
|---|---|---|
| 1 | `src/daphne_client.cpp`: `DaphneClient::Impl::Poll` | Try one native v8 snapshot; delegate to the isolated `PollLegacy` two-read adapter only if v8 is unavailable. |
| 2 | `src/v8_snapshot.cpp`: `MakeRequest`, `ParseAndValidate`, `VisitWireSamples` | Build request 1002 and validate schema 2.0. Walk the 316 declared `BoardTelemetry` fields, render their explicit instance keys, and reject bad annotations, board/sequence identity, duplicates, or value/quality combinations. |
| 3 | `src/opcua_snapshot_writer.cpp`: `OpcUaSnapshotWriter::Publish` | Convert only the Protobuf-declared typed samples, qualities, and source timestamps into OPC-UA DataValues. |
| 4 | `src/main.cpp`: `updateNodes` | Select current/last-good state, invoke the writer, and publish bridge-owned status. |

The mirrored schemas used to compile this bridge are in `../proto/upstream`.
Their canonical source is the `daphneZMQ/srcs/protobuf` directory. See
`../proto/upstream/README.md` before changing either copy.

The rest of `main.cpp` is not part of native DAPHNE telemetry decoding. It
implements configuration loading, namespace construction, power-supply I/O,
authentication, and controlled methods.

## Publication ownership

`OpcUaSnapshotWriter` publishes only samples declared and carried by
`BoardTelemetry`. Its 316 named fields expand to 1,370 board samples. The
producer contract excludes gateway-owned `Status.*` and `Bridge.*`
variables, so transport freshness and bridge health remain calculated in
`main.cpp`. For an older board, `main.cpp` publishes the small legacy
bias/rail subset instead. Native and legacy publication are mutually exclusive
within a polling cycle.

The bridge owns no second list of DAPHNE wire variables. Each field's compiled
Protobuf descriptor supplies its stable field number, scalar type, OPC-UA
NodeId pattern, engineering unit, data source, and control owner. Repeated
fields carry their instance keys explicitly. `VisitWireSamples` is therefore
a mechanical projection of the wire contract, not a place to add DAPHNE
semantics.

Current firmware replaces spy-buffer dead time with
`Spy.Trigger.SourceSelector` and `Spy.Trigger.Inhibit`. The bridge treats those
as ordinary typed DAQ-owned readbacks; it has no dead-time compatibility node.

## Invariants retained by the refactor

- One snapshot request per polling cycle, with message IDs 1002/1003.
- Complete contract validation before publication.
- Exactly 316 declared wire fields and 1,370 expected board samples.
- Exact typed NodeIds rendered from Protobuf annotations and instance keys.
- Per-sample OPC-UA quality and source timestamp preservation.
- Legacy readback fallback for boards that do not yet support v8.
- No DAPHNE or power writes unless their existing gates allow them.

The v8 snapshot is a read request/response only. DAQ configuration messages
remain in the DAQ-owned high-level protobuf and `daphnemodules`. A bridge method
must not synthesize a configuration command that is absent from that contract.
