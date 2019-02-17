//! An HTTP server implementation powered by `hyper` and `tower-service`.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.2")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod drain;
mod error;
mod util;

pub mod net;
pub mod runtime;
pub mod server;
pub mod service;
pub mod tls;

#[doc(inline)]
pub use crate::{
    error::{Error, Result},
    server::HttpServer,
};
