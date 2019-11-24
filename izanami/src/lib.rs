//! Web application interface inspired from ASGI.

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

mod app;
mod events;
mod websocket;

pub mod h2;

pub use crate::{
    app::App, //
    events::{Events, PushEvents, WebSocketEvents},
    websocket::Message,
};

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
