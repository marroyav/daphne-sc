#!/usr/bin/env sh
set -eu

section() {
	printf '\n== %s ==\n' "$1"
}

read_file() {
	path="$1"
	if [ -r "$path" ]; then
		printf '%s: ' "$path"
		tr -d '\000' < "$path" || true
		printf '\n'
	else
		printf '%s: missing-or-unreadable\n' "$path"
	fi
}

section "host"
hostname || true
date -Is 2>/dev/null || date -u '+%Y-%m-%dT%H:%M:%SZ' || true
uname -a || true

section "firmware"
for path in \
	/sys/class/fpga_manager/fpga0/state \
	/sys/firmware/devicetree/base/firmware-name \
	/etc/os-release
do
	read_file "$path"
done

section "pl devices"
for dev in \
	/sys/bus/platform/devices/9c000000.i2c \
	/sys/bus/platform/devices/9c020000.axi_quad_spi \
	/dev/i2c-1 \
	/dev/i2c-2 \
	/dev/spidev3.0
do
	if [ -e "$dev" ]; then
		printf 'present %s\n' "$dev"
	else
		printf 'missing %s\n' "$dev"
	fi
done

section "remoteproc"
if [ -d /sys/class/remoteproc ]; then
	for rp in /sys/class/remoteproc/remoteproc*; do
		[ -e "$rp" ] || continue
		printf '%s\n' "$rp"
		read_file "$rp/name"
		read_file "$rp/state"
		read_file "$rp/firmware"
	done
else
	printf '/sys/class/remoteproc missing\n'
fi

section "rpmsg"
for path in /sys/bus/rpmsg /sys/class/rpmsg /dev/rpmsg* /dev/ttyRPMSG*; do
	if [ -e "$path" ]; then
		ls -ld "$path"
	fi
done

section "i2c buses"
for dev in /dev/i2c-*; do
	[ -e "$dev" ] || continue
	ls -l "$dev"
done

section "expected i2c addresses"
for bus in 1 2; do
	if command -v i2cdetect >/dev/null 2>&1 && [ -e "/dev/i2c-$bus" ]; then
		printf 'bus %s\n' "$bus"
		i2cdetect -y "$bus" 2>&1 || true
	fi
done

section "thermal"
for zone in /sys/class/thermal/thermal_zone*; do
	[ -e "$zone" ] || continue
	printf '%s\n' "$zone"
	read_file "$zone/type"
	read_file "$zone/temp"
done

section "services"
if command -v systemctl >/dev/null 2>&1; then
	for svc in daphne.service hermes.service firmware.service clockchip.service endpoint.service; do
		systemctl --no-pager --plain is-active "$svc" 2>/dev/null | awk -v s="$svc" '{print s ": " $0}' || true
	done
fi

section "kernel messages"
dmesg | grep -Ei 'remoteproc|rproc|r5|rpu|rpmsg|virtio|ipi|mailbox|fpga|spidev|i2c' | tail -n 120 || true
