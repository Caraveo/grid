//! Coordinator: job queue, verify, PoR earn (Phase 1 embedded server).

mod client;
mod server;

pub use client::CoordinatorClient;
pub use server::run_coordinator;
