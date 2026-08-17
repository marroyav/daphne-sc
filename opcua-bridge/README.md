# PDS OPC-UA Bridge

This directory contains the first C++ vertical-slice bridge for the NP04
DAPHNE/PDS slow-controls setup:

```text
legacy DAPHNE daphneServer ZMQ/protobuf + USB power supply -> C++ OPC-UA server
```

It is intentionally separate from the existing Rust `daphne-sc-server`. The
Rust server remains the DAPHNE board service; this bridge is the DCS-facing
projection that Ignition can later browse.

## Build

Install dependencies on Debian:

```sh
sudo apt-get install cmake g++ pkg-config libopen62541-1.4-dev \
  libzmq3-dev cppzmq-dev protobuf-compiler libprotobuf-dev
```

Build:

```sh
cmake -S opcua-bridge -B build/opcua-bridge
cmake --build build/opcua-bridge -j
```

## Safe Local Smoke Test

The bridge runs in fake mode if no config is provided:

```sh
build/opcua-bridge/pds-opcua-bridge
```

Browse locally with an OPC-UA client at:

```text
opc.tcp://localhost:4840
```

## NP04 Run Shape

From WSL:

```sh
ssh -J marroyav@lxtunnel.CERN.CH marroyav@np04-onl-004.CERN.CH
```

The helper script stages the local bridge source, builds it on
`np04-onl-004.CERN.CH`, and runs it in the foreground. Start with fake endpoints:

```sh
opcua-bridge/tools/onl_bridge.sh fake
```

To stage the Interface2 exports and run the reversible DAPHNE-015 v8 profile
on port 4841, use:

```sh
REMOTE_DIR=daphne-sc-opcua-v8-stage \
CONTRACT_DIR=../Interface2/interface-data/daphne/exports \
opcua-bridge/tools/onl_bridge.sh v8
```

The helper accepts only a relative `REMOTE_DIR` beginning with
`daphne-sc-opcua`, does not remove the staging directory, and leaves every
write gate disabled in the v8 profile. Stop the foreground test with Ctrl-C.

Then, from another WSL terminal, open the tunnel:

```sh
opcua-bridge/tools/onl_bridge.sh tunnel
```

Browse locally at `opc.tcp://localhost:4840`.

On `np04-onl-004.CERN.CH`, after copying or building the repository:

```sh
build/opcua-bridge/pds-opcua-bridge --config opcua-bridge/config/np04-onl-004.example.conf
```

If the USBTMC supply path is blocking or unavailable, use the DAPHNE-only
profile while debugging the supply separately:

```sh
build/opcua-bridge/pds-opcua-bridge --config opcua-bridge/config/np04-onl-004-daphne-only.example.conf
```

If direct access to port `4840` is blocked, use SSH forwarding from WSL:

```sh
ssh -J marroyav@lxtunnel.CERN.CH \
  -L 4840:localhost:4840 \
  marroyav@np04-onl-004.CERN.CH
```

Then browse:

```text
opc.tcp://localhost:4840
```

## Exposed Namespaces

The namespace URI is `urn:dune:pds:np04`. The first layout is:

- `PDS/NP04/DAPHNE/<board-id>/Status`
- `PDS/NP04/PowerSupply/USB0/Status`
- `PDS/NP04/PowerSupply/USB0/Commands`
- `PDS/NP04/Summary`

The DAPHNE provider talks to the deployed legacy C++ `daphneServer` on
`NP04-DAPHNE-015.CERN.CH:40001` using read-only `ControlEnvelopeV2` messages:

- `MT2_READ_TEST_REG_REQ` / `MT2_READ_TEST_REG_RESP` as the service heartbeat.
- `MT2_READ_GENERAL_INFO_REQ` / `MT2_READ_GENERAL_INFO_RESP` for VBIAS, rail,
  and temperature readbacks.

Power-supply monitoring uses SCPI-style USBTMC or serial commands.

The workbook-driven v8 draft namespace is `urn:dune:pds:daphne`. Set
`registry.tag_list_file` to the exported `tag_list.csv` and
`control.policy_file` to `opc_ua_control_policy.csv`. The bridge then:

- validates every registry row and declared data type;
- expands `{BoardId}`, `{Afe}`, `{Channel}`, and the other controlled instance
  placeholders;
- publishes read variables with exact canonical string NodeIds;
- creates every policy method, while keeping unmapped or globally disabled
  methods non-executable;
- reports unbacked variables as `BadWaitingForInitialData` instead of
  publishing invented zeroes as good data.

For the current HD inventory, one board expands to 1,416 read variables and 21
methods. Run the offline contract check before deployment:

```sh
build/opcua-bridge/pds-registry-check contract/tag_list.csv 383 362 21
```

After starting the server, verify the complete namespace and declared types:

```sh
build/opcua-bridge/pds-opcua-registry-smoke \
  opc.tcp://localhost:4841 contract/tag_list.csv 015 \
  --expect-writes-disabled
```

## Multiple DAPHNE Boards

The bridge can poll several independently addressed DAPHNE servers and expose
one OPC-UA subtree per board. Configure a comma-separated board list, then an
endpoint and route for each identifier:

```ini
daphne.enabled = true
daphne.fake = false
daphne.ids = 001, 002, 003, 004
daphne.001.endpoint = tcp://127.20.0.1:40001
daphne.002.endpoint = tcp://127.20.0.2:40001
daphne.003.endpoint = tcp://127.20.0.3:40001
daphne.004.endpoint = tcp://127.20.0.4:40001
daphne.001.route = mezz/0
daphne.002.route = mezz/0
daphne.003.route = mezz/0
daphne.004.route = mezz/0
daphne.worker_count = 16
```

The older single-board keys remain supported:

```ini
daphne.id = 015
daphne.endpoint = tcp://NP04-DAPHNE-015.CERN.CH:40001
daphne.route = mezz/0
```

The summary subtree reports `DaphneReadyCount`, `DaphneTotalCount`, and a
combined readiness message. A failure on one server therefore remains visible
without hiding the state of the other boards.

Polling runs in background workers and the OPC-UA server publishes cached
snapshots. This prevents a slow or unreachable board from blocking the OPC-UA
server loop. Set `daphne.worker_count` to bound the number of concurrent ZMQ
poll workers for a bridge shard.

## Local Ignition Test Stack

On the development machine, the simulator and bridge are managed as user
services:

```sh
systemctl --user status daphne-ignition-emulator.service
systemctl --user status daphne-ignition-bridge.service
```

The local bridge configuration is:

```text
~/.config/daphne-ignition-test/bridge.conf
```

Ignition connects to `opc.tcp://127.0.0.1:4840`. The `DAPHNE-test`
Perspective project uses a parameterized `DAPHNE Board` UDT and a reusable
board-detail view, so additional instances do not require duplicating the
entire screen. Power I/O and writes remain disabled in this local profile.

## Write Safety

Power-supply writes are rejected unless all of these are true in config:

- `power.writes_enabled = true`
- voltage and current min/max values are nonzero and ordered
- requested voltage/current is inside the configured bounds

DAPHNE methods are generated from the workbook policy. A method is executable
only when authentication is configured, `daphne.writes_enabled = true`, its
policy row is executable, its backend mapping matches a compiled adapter, and
the authenticated role is allowed. The DAPHNE-015 v8 test profile leaves all
writes disabled.

## DAPHNE-015 v8 Test

`config/np04-daphne-015-v8-test.example.conf` is a read-only profile for
running the bridge on `np04-onl-004` against the legacy DAPHNE service on
`NP04-DAPHNE-015.CERN.CH:40001`. It listens on test port 4841 and does not
replace or restart `daphne.service` on the board.

## NP04 Hardware Probe

Before enabling real endpoints, run the probe from WSL through the CERN jump
host:

```sh
ssh -J marroyav@lxtunnel.CERN.CH marroyav@np04-onl-004.CERN.CH \
  'bash -s' < opcua-bridge/tools/np04_probe.sh
```

Use the output to replace the example DAPHNE endpoint and power-supply device
path. Prefer stable `/dev/serial/by-id/...` names for serial supplies.

## Service Template

`deploy/pds-opcua-bridge.service.example` is a systemd template for running the
bridge on `np04-onl-004.CERN.CH`. Install it only after copying a reviewed
config to `/etc/pds-opcua-bridge/np04-onl-004.conf`, creating the `pds-sc`
service user, and adding udev permissions for the final USB power-supply
device.
