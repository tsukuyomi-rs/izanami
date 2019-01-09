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
    hyper::body::Payload as _Payload,
    izanami_http::{BufStream, HasTrailers, Upgradable},
};

type CritError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A struct that represents the stream of chunks from client.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl BufStream for RequestBody {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll_data()
    }

    fn size_hint(&self) -> izanami_http::SizeHint {
        let mut hint = izanami_http::SizeHint::new();
        if let Some(len) = self.0.content_length() {
            hint.set_upper(len);
        }
        hint
    }
}

impl HasTrailers for RequestBody {
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        self.0.poll_trailers()
    }
}

impl Upgradable for RequestBody {
    type Upgraded = hyper::upgrade::Upgraded;
    type Error = hyper::error::Error;
    type OnUpgrade = hyper::upgrade::OnUpgrade;

    fn on_upgrade(self) -> Self::OnUpgrade {
        self.0.on_upgrade()
    }
}
