//! GPU via nvidia-smi.

use super::super::common::{nvidia_smi, run_out};
pub(crate) fn get_gpu_name() -> String {
    let out = run_out(&nvidia_smi(), &["--query-gpu=name", "--format=csv,noheader"]);
    let name = out.trim();
    if name.is_empty() {
        return String::new();
    }
    if name.contains("RTX 3060") {
        return "RTX 3060".to_string();
    }
    name.replace("NVIDIA GeForce", "")
        .replace("Laptop GPU", "")
        .replace("GPU", "")
        .trim()
        .to_string()
}

pub(crate) fn get_gpu_usage() -> i32 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]);
    out.trim().parse::<i32>().unwrap_or(0)
}

pub(crate) fn get_gpu_temp() -> i32 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"]);
    out.trim().parse::<i32>().unwrap_or(0)
}

pub(crate) fn get_gpu_power() -> f64 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=power.draw", "--format=csv,noheader,nounits"]);
    out.trim().parse::<f64>().unwrap_or(0.0)
}

pub(crate) fn get_gpu_memory() -> (i32, i32) {
    let out = run_out(&nvidia_smi(), &["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"]);
    let parts: Vec<&str> = out.split(',').collect();
    if parts.len() == 2 {
        let used = parts[0].trim().parse::<i32>().unwrap_or(0);
        let total = parts[1].trim().parse::<i32>().unwrap_or(0);
        return (used, total);
    }
    (0, 0)
}
