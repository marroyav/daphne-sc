#!/usr/bin/env bash
set -euo pipefail

echo "== host =="
hostname -f || hostname
date -Is
uname -a

echo
echo "== network =="
ip -br addr || true
ip route || true

echo
echo "== daphne zmq endpoint =="
daphne_endpoint="${DAPHNE_ENDPOINT:-NP04-DAPHNE-015.CERN.CH}"
daphne_port="${DAPHNE_PORT:-40001}"
echo "checking ${daphne_endpoint}:${daphne_port}"
if command -v getent >/dev/null 2>&1 && ! getent hosts "${daphne_endpoint}" >/dev/null; then
  echo "DNS lookup failed for ${daphne_endpoint}"
elif command -v nc >/dev/null 2>&1; then
  nc -vz -w 2 "${daphne_endpoint}" "${daphne_port}" || true
elif command -v timeout >/dev/null 2>&1; then
  timeout 2 bash -c "</dev/tcp/${daphne_endpoint}/${daphne_port}" && echo "tcp open" || echo "tcp closed or blocked"
else
  echo "no nc/timeout available for tcp check"
fi

echo
echo "== usb =="
if command -v lsusb >/dev/null 2>&1; then
  lsusb || true
else
  echo "lsusb not installed"
fi

echo
echo "== slow-control device nodes =="
for pattern in /dev/usbtmc* /dev/ttyUSB* /dev/ttyACM* /dev/serial/by-id/* /dev/i2c-* /dev/spidev*; do
  compgen -G "${pattern}" >/dev/null || continue
  ls -l ${pattern} || true
done

echo
echo "== opcua bridge build/runtime deps =="
for cmd in cmake g++ protoc pkg-config; do
  printf "%-12s" "${cmd}"
  command -v "${cmd}" || true
done
pkg-config --modversion cppzmq 2>/dev/null || true
pkg-config --modversion libzmq 2>/dev/null || true

echo
echo "== candidate config reminders =="
echo "DAPHNE endpoint: tcp://${daphne_endpoint}:${daphne_port}"
echo "Power supply: set power.device to /dev/usbtmcN or /dev/serial/by-id/... after confirming the connected supply"
