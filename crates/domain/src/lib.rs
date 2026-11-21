//! The pure core of pastegrab: link understanding, format planning, file names and job states.
//! Nothing here touches files, sockets, processes or the clock; `clippy.toml` keeps it that way.

#![forbid(unsafe_code)]

pub mod job;
pub mod link;
pub mod media;
pub mod name;
pub mod plan;
pub mod profile;
