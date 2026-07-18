//! Container service exposure policy.
//!
//! A job service is never published to a LAN/WAN interface.  The one allowed
//! port is bound to loopback and is intended to be selected by an authenticated
//! encrypted GRID peer session, not by a host shell or an arbitrary proxy.

use anyhow::{bail, Result};

/// Fixed internal service port for every GRID job container.
/// Deliberately high and non-standard so it cannot be mistaken for the P2P,
/// coordinator, Docker, SSH, or genesis ports.
pub const GRID_CONTAINER_PORT: u16 = 41_783;

pub fn validate_container_port(port: u16) -> Result<()> {
    if port != GRID_CONTAINER_PORT {
        bail!(
            "container service port must be {GRID_CONTAINER_PORT}; arbitrary host ports are forbidden"
        );
    }
    Ok(())
}

/// A locator, not a public URL. Direct host connections are intentionally not
/// supported: the peer tunnel must authenticate the job assignment first.
pub fn public_endpoint_hint(port: u16) -> String {
    format!(
        "grid:// service locator · encrypted assigned-job tunnel only · container loopback port {port}"
    )
}
