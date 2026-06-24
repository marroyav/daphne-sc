use crate::rpu::RpuLinkStatus;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub struct SlowControlStatus {
    pub firmware: FirmwareStatus,
    pub rpu: RpuLinkStatus,
    pub i2c: Vec<I2cBusStatus>,
    pub clocks: ClockStatus,
    pub temperatures: Vec<TemperatureReading>,
    pub rails: Vec<RailReading>,
    pub services: Vec<ServiceStatus>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareStatus {
    pub loaded: bool,
    pub fpga_manager_state: Option<String>,
    pub overlay_name: Option<String>,
    pub build_id: Option<String>,
    pub pl_devices_present: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cBusStatus {
    pub bus: u8,
    pub path: String,
    pub devices: Vec<I2cDeviceStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cDeviceStatus {
    pub address: u8,
    pub name: String,
    pub present: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockStatus {
    pub endpoint_clock_source_controlled: Option<bool>,
    pub mmcm0_locked: Option<bool>,
    pub mmcm1_locked: Option<bool>,
    pub raw_endpoint_status: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemperatureReading {
    pub name: String,
    pub celsius: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RailReading {
    pub name: String,
    pub voltage_v: Option<f64>,
    pub current_a: Option<f64>,
    pub power_w: Option<f64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    pub name: String,
    pub active: bool,
    pub state: String,
}
