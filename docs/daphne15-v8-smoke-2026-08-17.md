# DAPHNE-015 v8 OPC-UA smoke test — 2026-08-17

## Scope and safety

The v8 draft bridge was built and run on `np04-onl-004` against the existing
read-only DAPHNE endpoint at `tcp://NP04-DAPHNE-015.CERN.CH:40001`.

- The live `daphne.service` was not restarted or replaced.
- `daphne.writes_enabled` and `power.writes_enabled` were false.
- All 21 canonical methods were verified non-executable.
- The bridge used test port 4841 and was stopped after the smoke test.
- The built test deployment remains staged at
  `/nfs/home/marroyav/daphne-sc-opcua-v8-20260817`.

The board service remained active with PID 2441 and listening on port 40001
after the test.

The final bridge revision was also exercised locally under AddressSanitizer and
UndefinedBehaviorSanitizer. The complete 1,437-node namespace smoke passed
without a sanitizer finding. After that hardening pass, the source was rebuilt
in the dated CERN staging directory and both the contract test and live
namespace smoke were repeated successfully before port 4841 was closed.

## Native v8 protobuf sidecar validation

The additive board producer and bridge path were subsequently tested end to
end without changing the production service:

```text
DAPHNE-015 telemetry-only sidecar :40002
  -> ControlEnvelopeV2 + daphne.telemetry.v8/1.0
  -> bridge on np04-onl-004 :4842
  -> urn:dune:pds:daphne OPC-UA namespace
```

The ARM64 sidecar was built against the target protobuf/ZeroMQ ABI, required no
newer than `GLIBC_2.34` and `GLIBCXX_3.4.30`, and was checksum-verified after
transfer. It used `--telemetry-only`, which skips I2C/SPI peripheral
initialization and exposes only the read-only v8 snapshot command.

The final traceable build and staging identifiers are:

```text
daphneZMQ source commit                 578346b46fb06d894a7a8678ea7f835302cefa01
daphne-sc bridge source commit          a8dc089
protobuf source SHA-256                 a7463652fb48eb37333d2a883b49b2376d72efd5695bc147b7f20f717f89cbdb
ARM bundle SHA-256                      e2ba9b964cc76107c622e0d0c471e97262285e5c2a15c7d5af28542aa559b876
ARM daphneServer-v8 SHA-256             416bd585e75c55ecce477535b0b9a6e60beb13df826c0fa1103d8a2e08007985
board staging directory                 /home/petalinux/daphne-v8-telemetry-578346b
```

`Firmware.ServerVersion` returned `578346b` through OPC-UA during the final
test, confirming that the tested binary was built from the clean producer
commit rather than an uncommitted workspace.

One snapshot contained exactly 1,370 unique board-owned points:

```text
points=1370 good=349 unavailable=1021 invalid=0
```

The OPC-UA conformance check again passed all nodes with every method disabled:

```text
namespace=urn:dune:pds:daphne ns=3 checked=1437 failures=0
read_nodes=1416 method_nodes=21
```

Representative native readbacks were:

```text
Status.Connected                         Good  true
Bridge.Version                           Good  0.4.0
Firmware.GitCommit                       Good  0x10B3D2A
Host.CpuLoad1Minute                      Good  0.450195
Host.MemoryAvailableBytes                Good  3818758144
Network.Interfaces.eth0.Ipv4Address      Good  10.73.137.16
Network.Interfaces.eth0.Carrier          Good  true
Timing.Mmcm0Locked                       Good  true
Thermal.Fans.0.PwmCommand                Good  255
HDMezz.0.BlockEnabled                    BadWaitingForInitialData
Authority.ExternalActivityPermit         BadWaitingForInitialData
```

The numerical values are observations from this smoke test, not approved
limits. HD-mezzanine data was intentionally unavailable because the sidecar did
not initialize or probe the I2C peripherals. The authority point remained
unavailable because it belongs to SC/DPS, not the DAPHNE producer.

Both test processes were stopped. The final check showed only production PID
2441 listening on port 40001; ports 40002 and 4842 were closed.

## Contract and namespace results

The staged build used the proposed-v8 exports:

- 383 registry patterns
- 362 read-only patterns
- 21 method patterns
- 1,416 expanded HD-board read nodes
- 21 expanded methods

Contract SHA-256 values used for the test:

```text
tag_list.csv                 566fabaf6275d3d01f8b04b7a4e5db05f7d6eb204c1c4c9431c70825bf623e7d
opc_ua_control_policy.csv    e91d7f846dc75260a72d1e1d6815b25e241739224a85649537b3a860c0c709a9
workbook                     87f872595e5ddde5e09fe2930903811e6538472d8f17d461a5e9b1a2bec0077a
```

The offline registry contract test passed. The live OPC-UA conformance smoke
then checked all 1,437 expanded nodes for existence, node class, and declared
data type:

```text
namespace=urn:dune:pds:daphne ns=3 checked=1437 failures=0
read_nodes=1416 method_nodes=21
```

## Real DAPHNE-015 readbacks

The canonical nodes returned:

```text
Status.Connected                         Good  true
Status.Success                           Good  true
Status.Message                           Good  legacy daphneServer V2 readback ok
Status.Quality                           Good  Good
Status.AgeSeconds                        Good  0.888535
AFE.Blocks.0.BiasVoltage                 Good  0.605763 V
AFE.Blocks.4.BiasVoltage                 Good  0 V
Power.BoardRails.Minus5VA.Voltage        Good -5.02795 V
Power.BoardRails.3V3PDS.Voltage          Good  3.29889 V
Power.BoardRails.1V8A.Voltage            Good  1.80146 V
Bridge.Version                           Good  0.4.0
Bridge.PollErrorCount                    Good  0
```

These are observational smoke-test samples, not approved alarm or interlock
limits.

## NP02 admission state

The four fields introduced from the NP02 interaction-matrix review exist with
the correct declared types, but no SC/DPS source is connected yet. They fail
safe rather than publishing false data as good:

```text
Authority.ExternalConstraintRevision     BadWaitingForInitialData
Authority.ExternalConstraintStateFresh   BadWaitingForInitialData
Authority.ExternalActivityPermit         BadWaitingForInitialData
Authority.ExternalActivityConflict       BadWaitingForInitialData
```

## Remaining deployment blockers

- Configure production certificates and encrypted OPC-UA security policies.
- Supply authenticated DAQ, SC, DPS, and expert identities.
- Connect the SC/DPS external-activity admission source and enforce its guard.
- Commission the richer native v8 collectors in the persistent board service;
  the sidecar test intentionally left live I2C/SPI monitoring disabled.
- Validate the final instance inventories from HWDB, especially SFPs, sensors,
  services, I2C devices, and HD/VD channel counts.
- Add approved alarm/interlock limits and historization configuration.
- Review mapped write operations jointly before enabling any write gate.

Until those items are complete, the dated deployment is a read-only integration
test and must not be installed as a persistent production service.
