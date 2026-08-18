# DAPHNE-015 explicit-v8 Protobuf smoke test — 2026-08-18

## Result

The schema-2.0 explicit-field path passed end to end on DAPHNE-015:

```text
DAPHNE firmware/registers
  -> telemetry-only daphneServer sidecar on :40002
  -> BoardTelemetry with 316 named Protobuf fields / 1,370 keyed samples
  -> ControlEnvelopeV2 over ZMQ
  -> pds-opcua-bridge on np04-onl-004 :4842
  -> urn:dune:pds:daphne OPC-UA namespace
```

The canonical `.proto` is the wire-semantics owner. Each `BoardTelemetry`
field declares its stable field number, typed sample, explicit instance keys
where needed, OPC-UA NodeId pattern, engineering unit, data source, and control
owner. The bridge used those compiled descriptors mechanically and required
the producer's schema SHA-256 to exactly match its local schema.

The complete OPC-UA conformance check passed:

```text
namespace=urn:dune:pds:daphne ns=3 checked=1437 failures=0
read_nodes=1416 method_nodes=21
```

The received board snapshot was complete:

```text
samples=1370 good=349 unavailable=1021 invalid=0
```

The 1,416 read nodes comprise 1,370 DAPHNE-produced samples and 46 nodes owned
by the bridge or external authorities. All 21 methods were verified
non-executable because every write gate in the test profile was disabled.

Representative explicit readbacks were:

```text
DAPHNE.Boards.015.Spy.Trigger.SourceSelector       Good  3
DAPHNE.Boards.015.Spy.Trigger.Inhibit              Good  false
DAPHNE.Boards.015.Firmware.ProtobufSchemaVersion   Good  daphne.telemetry.v8/2.0
DAPHNE.Boards.015.Firmware.ServerVersion           Good  c262397
DAPHNE.Boards.015.Bridge.BackendProtocolVersion    Good  ControlEnvelopeV2+daphne.telemetry.v8/2.0
DAPHNE.Boards.015.Status.Message                    Good  v8 explicit telemetry ok: samples=1370 good=349 unavailable=1021 invalid=0
```

`SourceSelector=3` is the firmware's backward-compatible reset selection and
`Inhibit=false` means capture is not inhibited. The deprecated spy-buffer
dead-time variable is absent from the contract.

## Compatibility and traceability

The producer was cross-built for DAPHNE with Protobuf 30.1.0. The bridge and
its descriptor-validation test were built on the CERN ONL host with Protobuf
3.5.0. This exercised both ends of the supported generator/API range.

```text
daphneZMQ source commit          c262397
daphne-sc source commit          7b04f09
Interface2 source commit         6c0b245
v8 protobuf SHA-256              c464409f3fc88f37432cf38d83eccbbd87da41bfe9c615810e0cd37732782469
Interface2 field-trace SHA-256   6f654bedc6b11b8bb0d3af1066220e1f3cc8960ec496693d749ef83afe996177
ARM bundle SHA-256               bcc25c1adfff441a3caad2a1dbec7d16ccd380eab76f90f612bce0d2da2f5237
ARM daphneServer SHA-256         0ed659cf967353b332205fe3526e1cfc33c8f9b554854a060b58423150c5e8dd
ONL bridge binary SHA-256        001bd06036f912b5ee6473d1c4d87742850d883c0f020b29056061696b890aed
tag_list.csv SHA-256             566fabaf6275d3d01f8b04b7a4e5db05f7d6eb204c1c4c9431c70825bf623e7d
control policy SHA-256           e91d7f846dc75260a72d1e1d6815b25e241739224a85649537b3a860c0c709a9
```

The schema generator reproduced the `.proto` and field trace byte-for-byte.
The Interface2 checker matched every active field's pattern, type, unit, and
owner to the workbook export. Retired ledger entries remain reserved so a
later field cannot reuse their number or name.

The review artifacts remain staged at:

```text
np04-onl-004: /nfs/home/marroyav/daphne-sc-opcua-v8-explicit-20260818
DAPHNE-015:   /home/petalinux/daphne-v8-explicit-c262397
```

## Safety and teardown

- The production `/usr/bin/daphneServer` was not restarted or replaced.
- Production PID 2441 remained on port 40001 throughout the test.
- The sidecar used `--telemetry-only`, which exposes only snapshot request
  1002 and performs no I2C/SPI initialization.
- The OPC-UA test profile disabled DAPHNE and power-supply writes.
- The test bridge and board sidecar were stopped after validation.
- Final verification showed ports 4842 and 40002 closed, no test process
  remaining, and production PID 2441 still listening on port 40001.

This was a reversible commissioning test, not a production deployment. The
test OPC-UA endpoint allowed anonymous, unencrypted reads and must not be
installed as a persistent service configuration.
