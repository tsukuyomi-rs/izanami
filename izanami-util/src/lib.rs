//! Miscellaneous primitives used within izanami.

#![doc(html_root_url = "https://docs.rs/izanami-util/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

pub mod buf_stream;
pub mod http;
pub mod rt;

mod either;
mod remote;
mod sni;
mod util;

pub use crate::{
    either::Either, //
    remote::RemoteAddr,
    sni::SniHostname,
};
