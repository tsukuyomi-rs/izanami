//! An HTTP server implementation powered by `hyper` and `tower-service`.

#![doc(html_root_url = "https://docs.rs/izanami-server/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod drain;

pub mod request;
pub mod server;

#[allow(dead_code)]
type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub use crate::server::{MakeConnection, Server};
