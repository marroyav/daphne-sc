#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'USAGE'
usage: deploy/install-daphne-sc.sh [--units]

Copies prebuilt Rust slow-control binaries into /usr/local/bin.

Environment:
  PREFIX=/usr/local
  UNIT_DIR=/etc/systemd/system
  ENV_FILE=/etc/daphne-sc.env
  TARGET_TRIPLE=aarch64-unknown-linux-gnu

Build first with:
  cargo build --release --bins
USAGE
}

INSTALL_UNITS=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --units)
            INSTALL_UNITS=1
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
UNIT_DIR="${UNIT_DIR:-/etc/systemd/system}"
ENV_FILE="${ENV_FILE:-/etc/daphne-sc.env}"

if [[ -n "${TARGET_TRIPLE:-}" ]]; then
    BIN_DIR="$ROOT_DIR/target/$TARGET_TRIPLE/release"
else
    BIN_DIR="$ROOT_DIR/target/release"
fi

for binary in daphne-sc-server clockchip_tool mmio_smoke; do
    if [[ ! -x "$BIN_DIR/$binary" ]]; then
        echo "missing executable: $BIN_DIR/$binary" >&2
        echo "build first with: cargo build --release --bins" >&2
        exit 1
    fi
done

install -Dm755 "$BIN_DIR/daphne-sc-server" "$PREFIX/bin/daphne-sc-server"
install -Dm755 "$BIN_DIR/clockchip_tool" "$PREFIX/bin/clockchip_tool"
install -Dm755 "$BIN_DIR/mmio_smoke" "$PREFIX/bin/mmio_smoke"

if [[ ! -e "$ENV_FILE" ]]; then
    install -Dm644 "$ROOT_DIR/deploy/daphne-sc.env.example" "$ENV_FILE"
fi

if [[ "$INSTALL_UNITS" -eq 1 ]]; then
    install -Dm644 "$ROOT_DIR/deploy/systemd/daphne-sc.service" "$UNIT_DIR/daphne-sc.service"
    install -Dm644 "$ROOT_DIR/deploy/systemd/clockchip.service" "$UNIT_DIR/clockchip.service"
    install -Dm644 "$ROOT_DIR/deploy/systemd/daphne-mmio-smoke.service" "$UNIT_DIR/daphne-mmio-smoke.service"
    systemctl daemon-reload
fi

echo "installed daphne-sc-server, clockchip_tool, and mmio_smoke under $PREFIX/bin"
if [[ "$INSTALL_UNITS" -eq 1 ]]; then
    echo "installed systemd units under $UNIT_DIR"
fi
