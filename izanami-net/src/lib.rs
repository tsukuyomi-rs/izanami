//! Network abstraction for izanami.

#![doc(html_root_url = "https://docs.rs/izanami-net/0.1.0-preview.1")]
#![deny(
    //missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod sleep_on_errors;
mod util;

pub mod ext;
pub mod tcp;
pub mod tls;
pub mod unix;
