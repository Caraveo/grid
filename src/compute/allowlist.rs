//! Allowlisted container images for host path.

use anyhow::Result;
use std::fs;
use std::path::Path;

pub const DEFAULT_IMAGES: &[&str] = &[
    "alpine:3.20",
    "alpine:latest",
    "busybox:latest",
    "busybox:1.36",
    "hashicorp/http-echo:latest",
    "hashicorp/http-echo:1.0",
    "nginx:alpine",
    "nginx:1.27-alpine",
];

pub fn ensure_allowlist(config_dir: &Path) -> Result<()> {
    let root = config_dir.join("computes");
    fs::create_dir_all(&root)?;
    let path = root.join("allowlist.toml");
    if path.exists() {
        return Ok(());
    }
    let mut body = String::from("# GRID host image allowlist — digests or tags\nimages = [\n");
    for img in DEFAULT_IMAGES {
        body.push_str(&format!("  \"{img}\",\n"));
    }
    body.push_str("]\n");
    fs::write(path, body)?;
    Ok(())
}

pub fn is_image_allowed(config_dir: &Path, image: &str) -> Result<bool> {
    ensure_allowlist(config_dir)?;
    let path = config_dir.join("computes/allowlist.toml");
    let raw = fs::read_to_string(path)?;
    let image = image.trim();
    // Minimal TOML parse: collect quoted strings after images
    for line in raw.lines() {
        let t = line.trim();
        if let Some(rest) = t
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix("\",").or_else(|| s.strip_suffix('"')))
        {
            if rest == image || image_matches(rest, image) {
                return Ok(true);
            }
        }
    }
    // Also allow exact default list even if file edited oddly
    Ok(DEFAULT_IMAGES
        .iter()
        .any(|d| *d == image || image_matches(d, image)))
}

fn image_matches(allowed: &str, image: &str) -> bool {
    // allow repo without tag if allowlist has repo:tag of same repo
    let a = allowed.split('@').next().unwrap_or(allowed);
    let i = image.split('@').next().unwrap_or(image);
    if a == i {
        return true;
    }
    let a_repo = a.split(':').next().unwrap_or(a);
    let i_repo = i.split(':').next().unwrap_or(i);
    a_repo == i_repo && (a.ends_with(":latest") || i.ends_with(":latest"))
}
