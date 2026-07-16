//! Live host metrics for status / PoR inputs.

use anyhow::Result;
use sysinfo::System;

#[derive(Debug, Clone)]
pub struct HostMetrics {
    pub cpu_cores: usize,
    pub cpu_usage_pct: f32,
    pub memory_total_gb: f64,
    pub memory_used_gb: f64,
    pub cpu_flops_est: f64,
}

pub fn collect() -> Result<HostMetrics> {
    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu_cores = sys.cpus().len().max(1);
    let cpu_usage_pct = sys.global_cpu_info().cpu_usage();
    let memory_total_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let memory_used_gb = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let cpu_flops_est = cpu_cores as f64 * 3.0 * 8.0 * 0.5;
    Ok(HostMetrics {
        cpu_cores,
        cpu_usage_pct,
        memory_total_gb,
        memory_used_gb,
        cpu_flops_est,
    })
}

pub fn print_summary() -> Result<()> {
    let m = collect()?;
    println!("=== GRID Resources ===");
    println!(
        "CPU:    {} cores · {:.0}% load · ~{:.1} GFLOPS est",
        m.cpu_cores, m.cpu_usage_pct, m.cpu_flops_est
    );
    println!(
        "Memory: {:.1} / {:.1} GB",
        m.memory_used_gb, m.memory_total_gb
    );
    Ok(())
}
