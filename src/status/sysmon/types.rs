//! Serialized sysmon structs.

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub read_rate: f64,
    pub write_rate: f64,
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub usage_pct: i32,
}

impl Default for DiskInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            read_rate: 0.0,
            write_rate: 0.0,
            total_gb: 0.0,
            used_gb: 0.0,
            free_gb: 0.0,
            usage_pct: 0,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct SysmonStatus {
    pub cpu_name: String,
    pub cpu_usage: i32,
    pub cpu_temp: i32,
    pub cpu_freq: f64,
    pub cpu_power: f64,
    pub gpu_name: String,
    pub gpu_usage: i32,
    pub gpu_temp: i32,
    pub gpu_power: f64,
    pub gpu_mem_used: i32,
    pub gpu_mem_total: i32,
    pub ram_name: String,
    pub ram_speed: String,
    pub ram_total: f64,
    pub ram_used: f64,
    pub disks: Vec<DiskInfo>,
    pub disk0: DiskInfo,
    pub disk1: DiskInfo,
}
