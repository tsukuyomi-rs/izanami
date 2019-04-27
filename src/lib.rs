//! A lightweight HTTP server interface for Rust.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.3")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]
#![cfg_attr(test, deny(warnings))]

pub mod blocking;
pub mod body;
pub mod context;
pub mod error;
pub mod handler;
pub mod launcher;
pub mod localmap;
