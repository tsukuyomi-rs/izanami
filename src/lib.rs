//! A *meta* library for creating Web frameworks.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod error;
pub mod server;
pub mod test;

#[doc(inline)]
pub use crate::{
    error::{Error, Result},
    server::Server,
};

#[doc(no_inline)]
pub use {
    izanami_http as http, //
    izanami_rt as rt,
    izanami_service as service,
};

#[allow(dead_code)] // ?
type CritError = Box<dyn std::error::Error + Send + Sync + 'static>;
