//! Network compatibility gate for operator participation.

use anyhow::{bail, Result};

pub const CURRENT_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

fn triple(version: &str) -> Result<(u64, u64, u64)> {
    let core = version
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");
    let mut parts = core.split('.');
    let major = parts.next().unwrap_or("").parse()?;
    let minor = parts.next().unwrap_or("").parse()?;
    let patch = parts.next().unwrap_or("").parse()?;
    if parts.next().is_some() {
        bail!("invalid semantic version: {version}");
    }
    Ok((major, minor, patch))
}

pub fn meets_minimum(current: &str, minimum: &str) -> Result<bool> {
    Ok(triple(current)? >= triple(minimum)?)
}

pub fn require_minimum(current: &str, minimum: &str) -> Result<()> {
    if !meets_minimum(current, minimum)? {
        bail!(
            "GRID CLI {current} is below the network minimum {minimum}. Update before hosting or mining: curl -fsSL https://grid-compute.com/downloads/install.sh | bash"
        );
    }
    Ok(())
}

pub fn configured_minimum() -> String {
    std::env::var("GRID_MIN_CLI_VERSION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| CURRENT_CLI_VERSION.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions() {
        assert!(meets_minimum("0.2.24", "0.2.24").unwrap());
        assert!(meets_minimum("0.3.0", "0.2.24").unwrap());
        assert!(!meets_minimum("0.2.23", "0.2.24").unwrap());
        assert!(!meets_minimum("0.1.99", "0.2.24").unwrap());
    }

    #[test]
    fn rejects_invalid_versions() {
        assert!(meets_minimum("dev", "0.2.24").is_err());
        assert!(meets_minimum("0.2.24", "latest").is_err());
    }
}
