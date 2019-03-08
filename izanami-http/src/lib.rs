//! HTTP-specific abstraction for izanami.

#![doc(html_root_url = "https://docs.rs/izanami-http/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod service;

pub mod body;
pub mod upgrade;

pub use crate::{
    body::HttpBody, //
    service::HttpService,
    upgrade::Upgrade,
};
