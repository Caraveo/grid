//! Public exposure hints (cloudflared when present).

use std::process::Command;

/// Hint for public computes — prefer cloudflared, else localhost note.
pub fn public_endpoint_hint(port: u16) -> String {
    if which("cloudflared") {
        format!(
            "public: run `cloudflared tunnel --url http://127.0.0.1:{port}` (or wire named tunnel)"
        )
    } else {
        format!(
            "public: bind service on 127.0.0.1:{port}; install cloudflared for HTTPS tunnel (default public mode)"
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
