//! HTTP-specific abstraction for izanami.
//!
//! The types and traits provided by this crate intentionally imitates
//! the unreleased `tower-http-service`, and there will be replaced with
//! them in the future version.

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

pub mod body;
pub mod response;
pub mod service;

pub use crate::{
    body::HttpBody, //
    service::HttpService,
};
