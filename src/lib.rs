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

mod watch;

pub mod h1;
pub mod h2;
pub mod net;
pub mod rt;
pub mod server;

#[doc(no_inline)]
pub use {
    izanami_http as http, //
    izanami_service as service,
};

#[allow(dead_code)]
type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;
