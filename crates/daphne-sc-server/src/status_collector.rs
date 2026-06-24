use crate::endpoint::read_endpoint_register_status;
use crate::i2c::i2c_bus_candidates;
use crate::preflight::PreflightStatus;
use daphne_sc_core::rpu::RpuLinkStatus;
use daphne_sc_core::status::{
    ClockStatus, FirmwareStatus, I2cBusStatus, I2cDeviceStatus, RailReading, ServiceStatus,
    SlowControlStatus, TemperatureReading,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FPGA_MANAGER_STATE: &str = "/sys/class/fpga_manager/fpga0/state";

const SERVICES: &[&str] = &[
    "firmware.service",
    "clockchip.service",
    "endpoint.service",
    "hermes.service",
    "daphne.service",
];

const PL_DEVICES: &[&str] = &[
    "/sys/bus/platform/devices/9c000000.i2c",
    "/sys/bus/platform/devices/9c020000.axi_quad_spi",
];

pub fn collect_slow_control_status(
    preflight: &PreflightStatus,
    rpu: RpuLinkStatus,
) -> SlowControlStatus {
    let mut errors = preflight_errors(preflight);
    let clocks = collect_clock_status(&mut errors);

    SlowControlStatus {
        firmware: collect_firmware_status(),
        rpu,
        i2c: collect_i2c_status(preflight),
        clocks,
        temperatures: collect_temperatures(&mut errors),
        rails: collect_rails(&mut errors),
        services: collect_service_statuses(),
        errors,
    }
}

fn collect_firmware_status() -> FirmwareStatus {
    let fpga_manager_state = read_trimmed(FPGA_MANAGER_STATE);
    let env = read_board_config();
    let overlay_name = env
        .get("FIRMWARE_APP")
        .or_else(|| env.get("APP"))
        .cloned()
        .filter(|value| !value.is_empty());
    let build_id = overlay_name
        .as_ref()
        .and_then(|name| name.rsplit_once('_').map(|(_, suffix)| suffix.to_string()));
    let pl_devices_present = PL_DEVICES
        .iter()
        .filter(|path| Path::new(path).exists())
        .map(|path| (*path).to_string())
        .collect::<Vec<_>>();

    FirmwareStatus {
        loaded: fpga_manager_state.as_deref() == Some("operating"),
        fpga_manager_state,
        overlay_name,
        build_id,
        pl_devices_present,
    }
}

fn collect_i2c_status(preflight: &PreflightStatus) -> Vec<I2cBusStatus> {
    let mut bus_map = i2c_bus_candidates(None)
        .into_iter()
        .map(|bus| {
            (
                bus,
                I2cBusStatus {
                    bus,
                    path: format!("/dev/i2c-{bus}"),
                    devices: Vec::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for (bus, address) in sysfs_i2c_devices() {
        let entry = bus_map.entry(bus).or_insert_with(|| I2cBusStatus {
            bus,
            path: format!("/dev/i2c-{bus}"),
            devices: Vec::new(),
        });
        entry.devices.push(I2cDeviceStatus {
            address,
            name: sysfs_i2c_device_name(bus, address).unwrap_or_else(|| format!("0x{address:02X}")),
            present: true,
            status: Some("registered".to_string()),
        });
    }

    if let Some((bus, detail)) = clockchip_detail(preflight) {
        let entry = bus_map.entry(bus).or_insert_with(|| I2cBusStatus {
            bus,
            path: format!("/dev/i2c-{bus}"),
            devices: Vec::new(),
        });
        upsert_i2c_device(
            &mut entry.devices,
            I2cDeviceStatus {
                address: 0x70,
                name: "clockchip".to_string(),
                present: true,
                status: Some(detail),
            },
        );
    }

    bus_map
        .into_values()
        .map(|mut bus| {
            bus.devices.sort_by_key(|device| device.address);
            bus
        })
        .collect()
}

fn collect_clock_status(errors: &mut Vec<String>) -> ClockStatus {
    match read_endpoint_register_status() {
        Ok(status) => ClockStatus {
            endpoint_clock_source_controlled: Some(status.endpoint_clock_source()),
            mmcm0_locked: Some(status.mmcm0_locked()),
            mmcm1_locked: Some(status.mmcm1_locked()),
            raw_endpoint_status: Some(status.endpoint_status),
        },
        Err(err) => {
            errors.push(format!("endpoint register read failed: {err}"));
            ClockStatus {
                endpoint_clock_source_controlled: None,
                mmcm0_locked: None,
                mmcm1_locked: None,
                raw_endpoint_status: None,
            }
        }
    }
}

fn collect_temperatures(errors: &mut Vec<String>) -> Vec<TemperatureReading> {
    let mut readings = Vec::new();

    for zone in glob_paths("/sys/class/thermal", "thermal_zone") {
        let name = read_trimmed(zone.join("type")).unwrap_or_else(|| {
            zone.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("thermal_zone")
                .to_string()
        });
        match read_millidegrees(zone.join("temp")) {
            Ok(celsius) => readings.push(TemperatureReading { name, celsius }),
            Err(err) => errors.push(format!("thermal temperature read failed: {err}")),
        }
    }

    for hwmon in glob_paths("/sys/class/hwmon", "hwmon") {
        let hwmon_name = read_trimmed(hwmon.join("name")).unwrap_or_else(|| {
            hwmon
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("hwmon")
                .to_string()
        });
        for input in indexed_inputs(&hwmon, "temp") {
            let label = read_trimmed(hwmon.join(format!("temp{}_label", input.index)))
                .unwrap_or_else(|| format!("temp{}", input.index));
            match read_millidegrees(&input.path) {
                Ok(celsius) => readings.push(TemperatureReading {
                    name: format!("{hwmon_name}/{label}"),
                    celsius,
                }),
                Err(err) => errors.push(format!("hwmon temperature read failed: {err}")),
            }
        }
    }

    readings
}

fn collect_rails(errors: &mut Vec<String>) -> Vec<RailReading> {
    let mut rails = Vec::new();

    for hwmon in glob_paths("/sys/class/hwmon", "hwmon") {
        let hwmon_name = read_trimmed(hwmon.join("name")).unwrap_or_else(|| {
            hwmon
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("hwmon")
                .to_string()
        });

        for input in indexed_inputs(&hwmon, "in") {
            let label = read_trimmed(hwmon.join(format!("in{}_label", input.index)))
                .unwrap_or_else(|| format!("in{}", input.index));
            match read_scaled(&input.path, 1000.0) {
                Ok(voltage_v) => rails.push(RailReading {
                    name: format!("{hwmon_name}/{label}"),
                    voltage_v: Some(voltage_v),
                    current_a: None,
                    power_w: None,
                    status: Some("hwmon".to_string()),
                }),
                Err(err) => errors.push(format!("hwmon voltage read failed: {err}")),
            }
        }

        for input in indexed_inputs(&hwmon, "curr") {
            let label = read_trimmed(hwmon.join(format!("curr{}_label", input.index)))
                .unwrap_or_else(|| format!("curr{}", input.index));
            match read_scaled(&input.path, 1000.0) {
                Ok(current_a) => rails.push(RailReading {
                    name: format!("{hwmon_name}/{label}"),
                    voltage_v: None,
                    current_a: Some(current_a),
                    power_w: None,
                    status: Some("hwmon".to_string()),
                }),
                Err(err) => errors.push(format!("hwmon current read failed: {err}")),
            }
        }

        for input in indexed_inputs(&hwmon, "power") {
            let label = read_trimmed(hwmon.join(format!("power{}_label", input.index)))
                .unwrap_or_else(|| format!("power{}", input.index));
            match read_scaled(&input.path, 1_000_000.0) {
                Ok(power_w) => rails.push(RailReading {
                    name: format!("{hwmon_name}/{label}"),
                    voltage_v: None,
                    current_a: None,
                    power_w: Some(power_w),
                    status: Some("hwmon".to_string()),
                }),
                Err(err) => errors.push(format!("hwmon power read failed: {err}")),
            }
        }
    }

    rails
}

fn collect_service_statuses() -> Vec<ServiceStatus> {
    SERVICES
        .iter()
        .map(|service| service_status(service))
        .collect()
}

fn service_status(service: &str) -> ServiceStatus {
    let active_output = Command::new("systemctl")
        .args(["is-active", service])
        .output();
    let state = active_output
        .as_ref()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|err| format!("systemctl failed: {err}"));
    ServiceStatus {
        name: service.to_string(),
        active: active_output
            .as_ref()
            .map(|output| output.status.success() && state == "active")
            .unwrap_or(false),
        state,
    }
}

fn read_board_config() -> HashMap<String, String> {
    let mut values = HashMap::new();
    for path in ["/etc/default/firmware", "/etc/daphne-board.env"] {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            values.insert(key.trim().to_string(), unquote(value.trim()));
        }
    }
    values
}

fn preflight_errors(preflight: &PreflightStatus) -> Vec<String> {
    preflight
        .checks()
        .iter()
        .filter(|check| !check.ok)
        .map(|check| format!("{}: {}", check.name, check.detail))
        .collect()
}

fn clockchip_detail(preflight: &PreflightStatus) -> Option<(u8, String)> {
    preflight
        .checks()
        .iter()
        .find(|check| check.name == "clockchip_i2c" && check.ok)
        .and_then(|check| {
            parse_i2c_bus_from_detail(&check.detail).map(|bus| (bus, check.detail.clone()))
        })
}

fn parse_i2c_bus_from_detail(detail: &str) -> Option<u8> {
    let (_, rest) = detail.split_once("i2c-")?;
    let digits = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn sysfs_i2c_devices() -> BTreeSet<(u8, u8)> {
    let mut devices = BTreeSet::new();
    let Ok(entries) = fs::read_dir("/sys/bus/i2c/devices") else {
        return devices;
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some((bus, addr)) = name.split_once('-') else {
            continue;
        };
        let Ok(bus) = bus.parse::<u8>() else {
            continue;
        };
        let Ok(address) = u8::from_str_radix(addr, 16) else {
            continue;
        };
        devices.insert((bus, address));
    }

    devices
}

fn sysfs_i2c_device_name(bus: u8, address: u8) -> Option<String> {
    read_trimmed(format!("/sys/bus/i2c/devices/{bus}-{address:04x}/name"))
}

fn upsert_i2c_device(devices: &mut Vec<I2cDeviceStatus>, device: I2cDeviceStatus) {
    if let Some(existing) = devices
        .iter_mut()
        .find(|existing| existing.address == device.address)
    {
        *existing = device;
    } else {
        devices.push(device);
    }
}

struct IndexedInput {
    index: u32,
    path: PathBuf,
}

fn indexed_inputs(dir: &Path, prefix: &str) -> Vec<IndexedInput> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut inputs = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let number = name
                .strip_prefix(prefix)?
                .strip_suffix("_input")?
                .parse()
                .ok()?;
            Some(IndexedInput {
                index: number,
                path,
            })
        })
        .collect::<Vec<_>>();
    inputs.sort_by_key(|input| input.index);
    inputs
}

fn glob_paths(root: &str, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with(prefix))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn read_millidegrees(path: impl AsRef<Path>) -> Result<f64, String> {
    read_scaled(path, 1000.0)
}

fn read_scaled(path: impl AsRef<Path>, divisor: f64) -> Result<f64, String> {
    let path = path.as_ref();
    let raw = read_trimmed(path).ok_or_else(|| format!("{}: unreadable", path.display()))?;
    let value = raw
        .parse::<f64>()
        .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(value / divisor)
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|content| content.trim().to_string())
}

fn unquote(value: &str) -> String {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_i2c_bus_from_clockchip_detail() {
        assert_eq!(
            parse_i2c_bus_from_detail("0x70 present on /dev/i2c-2; 0xE6=0x06"),
            Some(2)
        );
    }
}
