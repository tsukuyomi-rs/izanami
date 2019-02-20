//! Components that abstracts asynchronous stream of bytes.
//!
//! The purpose of this crate is to provide a compatible layer for
//! [`tokio_buf`]. Therefore, the current implementation of this crate
//! stays just a simple imitation of `tokio_buf`.
//!
//! [`tokio_buf`]: https://tokio-rs.github.io/tokio/tokio_buf

#![doc(html_root_url = "https://docs.rs/izanami-buf/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod buf_stream;
mod size_hint;

pub mod ext;

pub use crate::{
    buf_stream::BufStream,
    ext::{BufStreamExt, FromBufStream},
    size_hint::SizeHint,
};
