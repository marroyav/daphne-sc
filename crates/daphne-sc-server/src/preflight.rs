use crate::i2c::{i2c_bus_candidates, LinuxI2cDevice};
use daphne_sc_core::clockchip::{
    ClockChipBus, CLOCKCHIP_DEFAULT_ADDR, CLOCKCHIP_SANITY_REGISTER, CLOCKCHIP_SANITY_VALUE,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightStatus {
    checks: Vec<PreflightCheck>,
}

impl PreflightStatus {
    pub fn new(checks: Vec<PreflightCheck>) -> Self {
        Self { checks }
    }

    pub fn ready() -> Self {
        Self { checks: Vec::new() }
    }

    pub fn ready_for_hardware_commands(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }

    pub fn checks(&self) -> &[PreflightCheck] {
        &self.checks
    }

    pub fn failure_message(&self) -> String {
        if self.ready_for_hardware_commands() {
            return "preflight ok".to_string();
        }

        let failures = self
            .checks
            .iter()
            .filter(|check| !check.ok)
            .map(|check| format!("{} ({})", check.name, check.detail))
            .collect::<Vec<_>>()
            .join("; ");
        format!("preflight failed: {failures}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl PreflightCheck {
    pub fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: true,
            detail: detail.into(),
        }
    }

    pub fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

pub trait PreflightProvider {
    fn check(&mut self) -> PreflightStatus;
}

pub struct LinuxPreflight {
    ttl: Duration,
    cached: Option<CachedPreflight>,
}

struct CachedPreflight {
    checked_at: Instant,
    status: PreflightStatus,
}

impl Default for LinuxPreflight {
    fn default() -> Self {
        Self {
            ttl: Duration::from_millis(1000),
            cached: None,
        }
    }
}

impl LinuxPreflight {
    pub fn with_ttl(ttl: Duration) -> Self {
        Self { ttl, cached: None }
    }

    fn collect() -> PreflightStatus {
        let config = read_board_config();
        let mut checks = vec![
            systemd_unit_active("firmware.service"),
            systemd_unit_active("clockchip.service"),
            systemd_unit_active("endpoint.service"),
            file_equals(
                "fpga_manager",
                "/sys/class/fpga_manager/fpga0/state",
                "operating",
            ),
            path_exists("pl_i2c_device", "/sys/bus/platform/devices/9c000000.i2c"),
            path_exists(
                "pl_spi_device",
                "/sys/bus/platform/devices/9c020000.axi_quad_spi",
            ),
            any_i2c_device(&config),
            path_exists("spi_device_node", "/dev/spidev3.0"),
            any_remoteproc_device(),
        ];
        checks.push(clockchip_reachable(&config));
        PreflightStatus::new(checks)
    }
}

impl PreflightProvider for LinuxPreflight {
    fn check(&mut self) -> PreflightStatus {
        if let Some(cached) = &self.cached {
            if cached.checked_at.elapsed() < self.ttl {
                return cached.status.clone();
            }
        }

        let status = Self::collect();
        self.cached = Some(CachedPreflight {
            checked_at: Instant::now(),
            status: status.clone(),
        });
        status
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

fn unquote(value: &str) -> String {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn systemd_unit_active(unit: &str) -> PreflightCheck {
    match Command::new("systemctl").args(["is-active", unit]).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if output.status.success() && stdout == "active" {
                PreflightCheck::ok(unit, "active")
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let detail = if stderr.is_empty() {
                    format!("state={stdout}")
                } else {
                    format!("state={stdout}, stderr={stderr}")
                };
                PreflightCheck::fail(unit, detail)
            }
        }
        Err(err) => PreflightCheck::fail(unit, format!("systemctl failed: {err}")),
    }
}

fn file_equals(name: &str, path: &str, expected: &str) -> PreflightCheck {
    match fs::read_to_string(path) {
        Ok(content) => {
            let actual = content.trim();
            if actual == expected {
                PreflightCheck::ok(name, format!("{path}={actual}"))
            } else {
                PreflightCheck::fail(name, format!("{path}={actual}, expected {expected}"))
            }
        }
        Err(err) => PreflightCheck::fail(name, format!("{path}: {err}")),
    }
}

fn path_exists(name: &str, path: &str) -> PreflightCheck {
    if Path::new(path).exists() {
        PreflightCheck::ok(name, path)
    } else {
        PreflightCheck::fail(name, format!("{path} is missing"))
    }
}

fn any_i2c_device(config: &HashMap<String, String>) -> PreflightCheck {
    let buses = clockchip_bus_candidates(config);
    if buses.is_empty() {
        return PreflightCheck::fail("i2c_device_node", "no /dev/i2c-* nodes found");
    }

    let detail = buses
        .iter()
        .map(|bus| format!("/dev/i2c-{bus}"))
        .collect::<Vec<_>>()
        .join(",");
    PreflightCheck::ok("i2c_device_node", detail)
}

fn any_remoteproc_device() -> PreflightCheck {
    match fs::read_dir("/sys/class/remoteproc") {
        Ok(mut entries) => {
            if entries.any(|entry| entry.is_ok()) {
                PreflightCheck::ok("remoteproc", "/sys/class/remoteproc")
            } else {
                PreflightCheck::fail("remoteproc", "/sys/class/remoteproc is empty")
            }
        }
        Err(err) => PreflightCheck::fail("remoteproc", format!("/sys/class/remoteproc: {err}")),
    }
}

fn clockchip_reachable(config: &HashMap<String, String>) -> PreflightCheck {
    let chip = parse_i2c_addr(&env_value(
        config,
        "CLOCKCHIP_ADDR",
        &format!("0x{CLOCKCHIP_DEFAULT_ADDR:02X}"),
    ))
    .unwrap_or(CLOCKCHIP_DEFAULT_ADDR);
    let buses = clockchip_bus_candidates(config);
    if buses.is_empty() {
        return PreflightCheck::fail("clockchip_i2c", "no candidate I2C buses");
    }

    let mut unavailable = Vec::new();
    for bus in &buses {
        match probe_i2c_addr(bus, &chip) {
            ProbeResult::Found(detail) => {
                return PreflightCheck::ok("clockchip_i2c", detail);
            }
            ProbeResult::Missing => {}
            ProbeResult::Unavailable(detail) => {
                unavailable.push(detail);
            }
        }
    }

    let candidates = buses
        .iter()
        .map(|bus| format!("/dev/i2c-{bus}"))
        .collect::<Vec<_>>()
        .join(",");
    if unavailable.is_empty() {
        PreflightCheck::fail(
            "clockchip_i2c",
            format!("{chip} not detected on {candidates}"),
        )
    } else {
        PreflightCheck::fail(
            "clockchip_i2c",
            format!(
                "0x{chip:02X} not reachable on {candidates}; probe errors: {}",
                unavailable.join("; ")
            ),
        )
    }
}

fn clockchip_bus_candidates(config: &HashMap<String, String>) -> Vec<u8> {
    let configured = env_value(config, "CLOCKCHIP_BUS", "auto");
    if !configured.is_empty() && configured != "auto" {
        return configured.parse().map(|bus| vec![bus]).unwrap_or_default();
    }

    i2c_bus_candidates(None)
}

fn env_value(config: &HashMap<String, String>, key: &str, default: &str) -> String {
    config
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

enum ProbeResult {
    Found(String),
    Missing,
    Unavailable(String),
}

fn probe_i2c_addr(bus: &u8, addr: &u16) -> ProbeResult {
    match LinuxI2cDevice::open(*bus, *addr) {
        Ok(mut device) => match device.read_register(CLOCKCHIP_SANITY_REGISTER) {
            Ok(value) if value == CLOCKCHIP_SANITY_VALUE => ProbeResult::Found(format!(
                "0x{addr:02X} present on /dev/i2c-{bus}; 0x{CLOCKCHIP_SANITY_REGISTER:02X}=0x{value:02X}"
            )),
            Ok(value) => ProbeResult::Unavailable(format!(
                "/dev/i2c-{bus} addr 0x{addr:02X} read 0x{CLOCKCHIP_SANITY_REGISTER:02X}=0x{value:02X}, expected 0x{CLOCKCHIP_SANITY_VALUE:02X}"
            )),
            Err(err) => ProbeResult::Unavailable(format!(
                "/dev/i2c-{bus} addr 0x{addr:02X} sanity read failed: {err}"
            )),
        },
        Err(err) => {
            let detail = err.to_string();
            if detail.contains("No such device") || detail.contains("Remote I/O") {
                ProbeResult::Missing
            } else {
                ProbeResult::Unavailable(format!("/dev/i2c-{bus} addr 0x{addr:02X}: {err}"))
            }
        }
    }
}

fn parse_i2c_addr(addr: &str) -> Option<u16> {
    let trimmed = addr.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u16>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_failed_checks() {
        let status = PreflightStatus::new(vec![
            PreflightCheck::ok("firmware.service", "active"),
            PreflightCheck::fail("clockchip.service", "state=failed"),
        ]);

        assert!(!status.ready_for_hardware_commands());
        assert_eq!(
            status.failure_message(),
            "preflight failed: clockchip.service (state=failed)"
        );
    }

    #[test]
    fn parses_hex_i2c_addresses() {
        assert_eq!(parse_i2c_addr("0x70"), Some(0x70));
        assert_eq!(parse_i2c_addr("112"), Some(112));
    }
}
