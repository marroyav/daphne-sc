# Deployment

The Rust stack is deployable without replacing the live `daphne.service` until
the board operator explicitly installs and starts the new units.

## Build

On a DAPHNE Linux image:

```bash
cargo build --release --bins
sudo deploy/install-daphne-sc.sh --units
```

For a cross build, set `TARGET_TRIPLE` when installing from the same checkout:

```bash
cargo build --release --target aarch64-unknown-linux-gnu --bins
sudo TARGET_TRIPLE=aarch64-unknown-linux-gnu deploy/install-daphne-sc.sh --units
```

The installer copies:

- `daphne-sc-server` to `/usr/local/bin/daphne-sc-server`
- `clockchip_tool` to `/usr/local/bin/clockchip_tool`
- `mmio_smoke` to `/usr/local/bin/mmio_smoke`
- `deploy/systemd/daphne-sc.service` to `/etc/systemd/system/daphne-sc.service`
- `deploy/systemd/clockchip.service` to `/etc/systemd/system/clockchip.service`
- `deploy/systemd/daphne-mmio-smoke.service` to
  `/etc/systemd/system/daphne-mmio-smoke.service`
- `deploy/daphne-sc.env.example` to `/etc/daphne-sc.env` only if that file does
  not already exist

## Environment

`daphne-sc.service`, `clockchip.service`, and the Rust preflight code read:

- `/etc/default/firmware`
- `/etc/daphne-board.env`
- `/etc/daphne-sc.env`

Useful Rust-specific settings:

```bash
DAPHNE_SC_BIND=tcp://*:40002
DAPHNE_SC_RPU_TIMEOUT_MS=200
# DAPHNE_SC_RPU_RPMSG=/dev/rpmsg_daphne_afe
CLOCKCHIP_BUS=2
CLOCKCHIP_ADDR=0x70
# ENDPOINT_SUCCESS_STATES=0x8
```

If `DAPHNE_SC_RPU_RPMSG` is unset, AFE commands still fail closed.

## Service Order

`clockchip.service` is a Rust replacement for the existing clock-chip service
name. It programs and verifies the timing chip after `firmware.service` and
before `endpoint.service`, `hermes.service`, `daphne.service`, and
`daphne-sc.service`.

`daphne-sc.service` requires `firmware.service`, `clockchip.service`, and
`endpoint.service`. The Rust server still performs its own preflight before any
AFE command reaches the RPU transport.

## Smoke Tests

Run these before switching clients over:

```bash
sudo /usr/local/bin/mmio_smoke --skip-spy
sudo systemctl start daphne-mmio-smoke.service
sudo journalctl -u daphne-mmio-smoke.service -n 80 --no-pager
sudo /usr/local/bin/clockchip_tool verify
/usr/local/bin/daphne-sc-server --bind-smoke tcp://*:41002
```

`mmio_smoke` is read-only. The service form skips spy-buffer reads for the first
minimal DAPHNE-15 test; a manual full read can be run with
`sudo /usr/local/bin/mmio_smoke` once the basic register window is confirmed.

Start the Rust server on the normal endpoint only when the legacy service is not
using the same port:

```bash
sudo systemctl restart clockchip.service
sudo systemctl start daphne-sc.service
```

## v8 OPC-UA integration test

The workbook-driven bridge has a separate read-only DAPHNE-015 test profile at
`opcua-bridge/config/np04-daphne-015-v8-test.example.conf`. It is intended to
run on `np04-onl-004`, not on the PetaLinux board, and uses OPC-UA port 4841.
The profile leaves every write gate disabled and consumes the exported
`tag_list.csv` and `opc_ua_control_policy.csv` from the proposed-v8 ICD.

Do not install it persistently until certificates, authenticated identities,
SC/DPS admission inputs, and production instance inventories are approved. The
2026-08-17 smoke-test evidence is recorded in
`docs/daphne15-v8-smoke-2026-08-17.md`.
