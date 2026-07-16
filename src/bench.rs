//! Minimal resource benchmarking for PoR / operator self-check.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::resources;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub cpu_cores: usize,
    pub duration_secs: f64,
    /// Blake3 hashing throughput (MB/s).
    pub hash_mibs: f64,
    /// Approximate hash ops per second (1 op = 4 KiB).
    pub hash_ops_per_sec: f64,
    /// Sequential fill + checksum of a buffer (MB/s).
    pub mem_mibs: f64,
    /// Composite 0..100 score for display / P2P hello.
    pub score: f64,
    pub memory_total_gb: f64,
}

/// Run a short CPU + memory benchmark.
pub fn run(duration: Duration) -> Result<BenchReport> {
    let host = resources::collect()?;
    let secs = duration.as_secs_f64().max(0.5);

    let hash = bench_hash(duration);
    let mem = bench_mem();

    // Weighted composite for a simple single number peers can compare.
    let score = (hash.mibs / 50.0 * 40.0 + mem.mibs / 2000.0 * 30.0 + host.cpu_cores as f64 * 2.0)
        .clamp(0.0, 100.0);

    Ok(BenchReport {
        cpu_cores: host.cpu_cores,
        duration_secs: secs,
        hash_mibs: hash.mibs,
        hash_ops_per_sec: hash.ops_per_sec,
        mem_mibs: mem.mibs,
        score,
        memory_total_gb: host.memory_total_gb,
    })
}

struct HashStats {
    mibs: f64,
    ops_per_sec: f64,
}

fn bench_hash(duration: Duration) -> HashStats {
    const CHUNK: usize = 4 * 1024;
    let mut buf = vec![0u8; CHUNK];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    let start = Instant::now();
    let mut ops: u64 = 0;
    while start.elapsed() < duration {
        // mix previous digest into buffer so work isn't optimized away
        let d = blake3::hash(&buf);
        buf[0] = buf[0].wrapping_add(d.as_bytes()[0]);
        ops += 1;
    }
    let elapsed = start.elapsed().as_secs_f64().max(1e-9);
    let bytes = ops as f64 * CHUNK as f64;
    HashStats {
        mibs: bytes / (1024.0 * 1024.0) / elapsed,
        ops_per_sec: ops as f64 / elapsed,
    }
}

struct MemStats {
    mibs: f64,
}

fn bench_mem() -> MemStats {
    // ~64 MiB sequential write + fold
    const N: usize = 64 * 1024 * 1024;
    let mut buf = vec![0u8; N];
    let start = Instant::now();
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xff) as u8;
    }
    let mut sum: u64 = 0;
    for b in &buf {
        sum = sum.wrapping_add(*b as u64);
    }
    std::hint::black_box(sum);
    let elapsed = start.elapsed().as_secs_f64().max(1e-9);
    // write + read ≈ 2 passes over N
    MemStats {
        mibs: (2.0 * N as f64) / (1024.0 * 1024.0) / elapsed,
    }
}

pub fn print_report(r: &BenchReport) {
    println!("=== GRID Benchmark ===");
    println!("CPU cores:     {}", r.cpu_cores);
    println!("Duration:      {:.2}s", r.duration_secs);
    println!("Hash:          {:.1} MiB/s  ({:.0} ops/s @ 4KiB)", r.hash_mibs, r.hash_ops_per_sec);
    println!("Memory seq:    {:.0} MiB/s", r.mem_mibs);
    println!("Host RAM:      {:.1} GB", r.memory_total_gb);
    println!("Composite:     {:.1} / 100", r.score);
    println!();
    println!("(Higher hash/mem = more useful-work capacity on this box.)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_runs_quickly() {
        let r = run(Duration::from_millis(200)).unwrap();
        assert!(r.hash_mibs > 0.0);
        assert!(r.mem_mibs > 0.0);
        assert!(r.score >= 0.0 && r.score <= 100.0);
    }
}
