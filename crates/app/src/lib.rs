//! The application: the ports adapters implement, the job queue that decides, the steps workers
//! run, and the runtime that ties them to threads. Nothing here touches files, processes or
//! sockets directly; `clippy.toml` keeps it that way.

#![forbid(unsafe_code)]

pub mod board;
pub mod cancel;
pub mod failure;
pub mod ports;
pub mod queue;
pub mod runtime;
pub mod steps;

pub use cancel::Cancel;
pub use failure::Failure;
