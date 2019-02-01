//! The *basic* utility for testing HTTP services.

#![doc(html_root_url = "https://docs.rs/izanami-test/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod error;
mod input;
mod output;
mod runtime;
mod server;
pub mod service;

pub use crate::{
    error::{Error, Result},
    input::Input,
    output::Output,
    runtime::Runtime,
    server::{Client, Server},
};

#[allow(dead_code)] // ?
type CritError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Creates a test server using the specified service factory.
pub fn server<S>(make_service: S) -> crate::Result<Server<S, tokio::runtime::Runtime>>
where
    S: crate::service::MakeTestService,
    tokio::runtime::Runtime: Runtime<S>,
{
    let mut builder = tokio::runtime::Builder::new();
    builder.core_threads(1);
    builder.blocking_threads(1);
    builder.name_prefix("izanami-test");
    let runtime = builder.build()?;

    Ok(Server::new(make_service, runtime))
}

/// Creates a test server that exexutes all task onto a single thread,
/// using the specified service factory.
pub fn local_server<S>(
    make_service: S,
) -> crate::Result<Server<S, tokio::runtime::current_thread::Runtime>>
where
    S: crate::service::MakeTestService,
    tokio::runtime::current_thread::Runtime: Runtime<S>,
{
    let runtime = tokio::runtime::current_thread::Runtime::new()?;
    Ok(Server::new(make_service, runtime))
}
