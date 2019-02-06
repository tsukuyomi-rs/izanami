//! A lightweight implementation of HTTP server based on Hyper.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.2")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod error;
pub mod io;
pub mod server;
pub mod service;
pub mod test;
pub mod tls;

#[doc(inline)]
pub use crate::{
    error::{Error, Result},
    server::Server,
};
