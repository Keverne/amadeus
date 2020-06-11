#![doc(html_root_url = "https://docs.rs/amadeus-core/0.2.0")]
#![feature(never_type)]
#![feature(specialization)]
#![feature(read_initializer)]

pub mod dist_pipe;
pub mod dist_sink;
pub mod dist_stream;
pub mod file;
pub mod into_dist_stream;
pub mod misc_serde;
pub mod pool;
pub mod sink;
mod source;
pub mod util;

pub use source::*;
