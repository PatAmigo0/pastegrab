//! The pure core of pastegrab: link understanding and format planning now, job states next.
//! Nothing here touches files, sockets, processes or the clock; `clippy.toml` keeps it that way.

#![forbid(unsafe_code)]

pub mod link;
pub mod media;
pub mod plan;
pub mod profile;
