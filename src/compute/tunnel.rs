//! Public exposure hints — P2P mesh first; tunnels are last resort.

use std::process::Command;

/// Hint for public computes. Prefer GRID P2P peer announce; cloudflared only if needed.
pub fn public_endpoint_hint(port: u16) -> String {
    if which("cloudflared") {
        format!(
            "prefer: grid peer (P2P data plane) · last-resort tunnel: `cloudflared tunnel --url http://127.0.0.1:{port}`"
        )
    } else {
        format!(
            "prefer: grid peer / P2P mesh for reachability · local service 127.0.0.1:{port} (cloudflared optional last resort)"
        )
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
