//! An HTTP server implementation powered by `hyper` and `tower-service`.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.2")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod drain;
mod error;
mod util;

pub mod net;
pub mod runtime;
pub mod server;
pub mod service;
pub mod tls;

pub use crate::{
    error::{Error, Result},
    server::Server,
};

use {
    crate::{
        runtime::Block,
        server::Incoming,
        service::{HttpService, MakeHttpService},
        tls::MakeTlsTransport,
    },
    futures::Future,
    izanami_service::StreamService,
    std::net::ToSocketAddrs,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// Start an HTTP server using an TCP listener bound to the specified address.
pub fn run<A, T, S>(addr: A, tls: T, make_service: S) -> crate::Result<()>
where
    A: ToSocketAddrs,
    T: MakeTlsTransport<crate::net::tcp::AddrStream> + Send + 'static,
    T::Future: Send + 'static,
    T::Transport: Send + 'static,
    S: MakeHttpService<crate::net::tcp::AddrStream> + Send + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
{
    let incoming = crate::net::tcp::AddrIncoming::bind(addr)?;
    run_incoming(Incoming::new(incoming, make_service, tls))
}

/// Start an HTTP server using an Unix domain socket listener bounded to the specified path.
#[cfg(unix)]
pub fn run_unix<P, T, S>(path: P, tls: T, make_service: S) -> crate::Result<()>
where
    P: AsRef<std::path::Path>,
    T: MakeTlsTransport<crate::net::unix::AddrStream> + Send + 'static,
    T::Future: Send + 'static,
    T::Transport: Send + 'static,
    S: MakeHttpService<crate::net::unix::AddrStream> + Send + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
{
    let incoming = crate::net::unix::AddrIncoming::bind(path)?;
    run_incoming(Incoming::new(incoming, make_service, tls))
}

fn run_incoming<S, T, C>(stream_service: S) -> crate::Result<()>
where
    S: StreamService<Response = (T, C)> + Send + 'static,
    S::Future: Send + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + Send + 'static,
    C::Future: Send + 'static,
{
    let mut rt = tokio::runtime::Runtime::new()?;

    let (server, handle) = crate::server::Server::builder(stream_service).build();
    server.spawn(&mut rt);

    let _ = handle.block(&mut rt);
    rt.shutdown_on_idle().wait().unwrap();

    Ok(())
}
