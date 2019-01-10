//! A lightweight implementation of HTTP server for Web frameworks.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod error;
mod io;
pub mod rt;
mod server;
pub mod test;

pub use crate::{
    error::{Error, Result},
    io::{Acceptor, Listener},
    server::Server,
};

use {
    futures::Poll,
    http::HeaderMap,
    izanami_http::{
        buf_stream::{BufStream, SizeHint}, //
        upgrade::OnUpgrade,
        HasTrailers,
    },
};

type CritError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A struct that represents the stream of chunks from client.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl BufStream for RequestBody {
    type Item = <hyper::Body as BufStream>::Item;
    type Error = <hyper::Body as BufStream>::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll_buf()
    }

    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }
}

impl HasTrailers for RequestBody {
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        self.0.poll_trailers()
    }
}

impl OnUpgrade for RequestBody {
    type Upgraded = <hyper::Body as OnUpgrade>::Upgraded;
    type Error = <hyper::Body as OnUpgrade>::Error;
    type Future = <hyper::Body as OnUpgrade>::Future;

    fn on_upgrade(self) -> Self::Future {
        self.0.on_upgrade()
    }
}
