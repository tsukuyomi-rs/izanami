//! A lightweight Web server interface for building Web frameworks.

#![doc(html_root_url = "https://docs.rs/izanami/0.2.0-dev")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]
#![cfg_attr(test, deny(warnings))]

pub mod body;
pub mod context;
pub mod error;
pub mod handler;
pub mod localmap;
pub mod middleware;
pub mod rt;
pub mod ws;

mod app;
mod launcher;
#[doc(hidden)]
pub mod server;
mod util;

#[doc(inline)]
pub use crate::{
    app::App, //
    context::Context,
    handler::Handler,
    launcher::Launcher,
    middleware::Middleware,
};

const VERSION_STR: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[test]
fn test_version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}
