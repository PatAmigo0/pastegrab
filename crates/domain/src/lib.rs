//! The pure core of pastegrab: link understanding first, format planning and job states next.
//! Nothing here touches files, sockets, processes or the clock; `clippy.toml` keeps it that way.

pub mod link;
