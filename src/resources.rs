//! Live host metrics for status / PoR inputs.

use anyhow::Result;
use sysinfo::{Disks, System};

#[derive(Debug, Clone)]
pub struct HostMetrics {
    pub cpu_model: String,
    pub cpu_physical_cores: usize,
    pub cpu_cores: usize,
    pub cpu_frequency_mhz: u64,
    pub cpu_usage_pct: f32,
    pub memory_total_gb: f64,
    pub memory_used_gb: f64,
    pub storage_total_gb: f64,
    pub storage_available_gb: f64,
    pub cpu_flops_est: f64,
}

pub fn collect() -> Result<HostMetrics> {
    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu_cores = sys.cpus().len().max(1);
    let cpu_physical_cores = sys.physical_core_count().unwrap_or(cpu_cores).max(1);
    let cpu_model = sys
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown CPU".into());
    let cpu_frequency_mhz = sys
        .cpus()
        .iter()
        .map(|cpu| cpu.frequency())
        .max()
        .unwrap_or(0);
    let cpu_usage_pct = sys.global_cpu_info().cpu_usage();
    let memory_total_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let memory_used_gb = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let disks = Disks::new_with_refreshed_list();
    let storage_total_gb =
        disks.iter().map(|disk| disk.total_space()).sum::<u64>() as f64 / 1_000_000_000.0;
    let storage_available_gb =
        disks.iter().map(|disk| disk.available_space()).sum::<u64>() as f64 / 1_000_000_000.0;
    // Conservative display estimate: physical cores × GHz × 8-wide vector × 50% utilization.
    let cpu_flops_est =
        cpu_physical_cores as f64 * (cpu_frequency_mhz as f64 / 1_000.0) * 8.0 * 0.5;
    Ok(HostMetrics {
        cpu_model,
        cpu_physical_cores,
        cpu_cores,
        cpu_frequency_mhz,
        cpu_usage_pct,
        memory_total_gb,
        memory_used_gb,
        storage_total_gb,
        storage_available_gb,
        cpu_flops_est,
    })
}

pub fn print_summary() -> Result<()> {
    let m = collect()?;
    println!("=== GRID Resources ===");
    println!(
        "CPU:    {} · {} physical / {} logical · {} MHz · {:.0}% load · ~{:.1} GFLOPS est",
        m.cpu_model,
        m.cpu_physical_cores,
        m.cpu_cores,
        m.cpu_frequency_mhz,
        m.cpu_usage_pct,
        m.cpu_flops_est
    );
    println!(
        "Memory: {:.1} / {:.1} GB",
        m.memory_used_gb, m.memory_total_gb
    );
    println!(
        "Storage: {:.1} / {:.1} GB available",
        m.storage_available_gb, m.storage_total_gb
    );
    Ok(())
}
