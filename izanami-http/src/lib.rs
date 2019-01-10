//! Basic abstractions around HTTP used within izanami.

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

pub mod buf_stream;
pub mod upgrade;
mod util;

use {
    crate::buf_stream::BufStream, //
    futures::{Async, Poll},
    http::HeaderMap,
};

/// A trait representing that it is possible that the stream
/// will return a `HeaderMap` after completing the output of bytes.
pub trait HasTrailers: BufStream {
    /// Polls if this stream is ready to return a `HeaderMap`.
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        Ok(Async::Ready(None))
    }
}

#[cfg(feature = "hyper")]
mod impl_hyper {
    use super::*;
    use hyper::body::Payload;

    impl HasTrailers for hyper::Body {
        fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
            Payload::poll_trailers(self)
        }
    }
}
