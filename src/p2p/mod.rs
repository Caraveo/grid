//! Minimal TCP P2P mesh — hello, ping/pong RTT, peer gossip.

mod protocol;
mod swarm;

pub use swarm::{run_peer, PeerOptions};
