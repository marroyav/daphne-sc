# DAPHNE-015 v8 trigger-selector smoke test — 2026-08-18

## Result

The refactored v8 telemetry path passed end to end on DAPHNE-015:

```text
DAPHNE firmware/registers
  -> telemetry-only daphneServer sidecar on :40002
  -> ControlEnvelopeV2 + daphne.telemetry.v8/1.0
  -> pds-opcua-bridge on np04-onl-004 :4842
  -> urn:dune:pds:daphne OPC-UA namespace
```

The complete registry check passed:

```text
namespace=urn:dune:pds:daphne ns=3 checked=1437 failures=0
read_nodes=1416 method_nodes=21
```

The two firmware trigger-control readbacks that replace spy-buffer dead time
were both typed and Good:

```text
DAPHNE.Boards.015.Spy.Trigger.SourceSelector  Good  3
DAPHNE.Boards.015.Spy.Trigger.Inhibit         Good  false
```

`SourceSelector=3` is the firmware's backward-compatible `LegacyOrAll` reset
selection. `Inhibit=false` means spy-buffer capture is not inhibited. The
deprecated spy-buffer dead-time variable is not part of the v8 catalog.

Additional traceability readbacks were:

```text
DAPHNE.Boards.015.Firmware.ServerVersion       Good  8f0bbe8
DAPHNE.Boards.015.Bridge.BackendProtocolVersion Good  ControlEnvelopeV2+daphne.telemetry.v8/1.0
DAPHNE.Boards.015.Status.Quality                Good  Good
```

## Tested revisions and artifacts

```text
daphneZMQ source commit        8f0bbe8
daphne-sc source commit        806aa6b
Interface2 source commit       f05d2a0
v8 protobuf SHA-256            a7463652fb48eb37333d2a883b49b2376d72efd5695bc147b7f20f717f89cbdb
ARM bundle SHA-256             6b33202272f835bdc55de4c546f87f33f4dabea9b0d55e598d740bd0cf712482
ARM daphneServer SHA-256       0ab7647643c6411ca50ad26d1cd43928e9b59eee05554cd7f1a45cc4a25058b4
ONL bridge binary SHA-256      54245fae54fb2423eabb04e1a248829d81d100859ad68810929b5f0c50e76e59
tag_list.csv SHA-256           566fabaf6275d3d01f8b04b7a4e5db05f7d6eb204c1c4c9431c70825bf623e7d
control policy SHA-256         e91d7f846dc75260a72d1e1d6815b25e241739224a85649537b3a860c0c709a9
```

The bridge validation test also passed on the CERN host with Protobuf 3.5.0.
The registry contained 383 patterns: 362 read-only patterns and 21 method
patterns. Every method was verified non-executable because all write gates in
the test profile were disabled.

The artifacts remain staged for review at:

```text
np04-onl-004: /nfs/home/marroyav/daphne-sc-opcua-v8-20260818
DAPHNE-015:   /home/petalinux/daphne-v8-telemetry-8f0bbe8
```

## Safety and teardown

- The production `/usr/bin/daphneServer` was not restarted or replaced.
- Production PID 2441 remained on port 40001 throughout the test.
- The sidecar used `--telemetry-only`; no slow-control writes were exposed.
- The OPC-UA profile disabled DAPHNE and power-supply writes.
- The temporary bridge and sidecar were stopped after the checks.
- Final verification showed ports 40002 and 4842 closed, with only production
  PID 2441 listening on port 40001.

This was a reversible commissioning smoke test, not a production deployment.
The test OPC-UA endpoint allowed anonymous, unencrypted reads and must not be
installed as the persistent service configuration.
