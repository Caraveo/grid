//! Coordinator: persistent job queue, verify, PoR earn (pilot fabric).

mod client;
mod server;

pub use client::CoordinatorClient;
pub use server::{run_coordinator, run_coordinator_with, CoordOptions};
