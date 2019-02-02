//! The basic utility for testing HTTP services.

#![doc(html_root_url = "https://docs.rs/izanami-test/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

pub mod client;
mod error;
mod output;
pub mod runtime;
mod server;
pub mod service;

pub use crate::{
    error::{Error, Result},
    output::Output,
    server::Server,
};
