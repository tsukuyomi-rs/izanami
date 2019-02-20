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
        runtime::Spawn,
        server::Incoming,
        service::{HttpService, NewHttpService},
        tls::MakeTlsTransport,
    },
    izanami_service::StreamService,
    std::net::ToSocketAddrs,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// Start an HTTP server using an TCP listener.
///
/// This function internally uses the multi-threaded Tokio runtime with the default configuration.
pub fn run_tcp<A, T, S>(addr: A, tls: T, make_service: S) -> crate::Result<()>
where
    A: ToSocketAddrs,
    T: MakeTlsTransport<crate::net::tcp::AddrStream> + Send + 'static,
    T::Future: Send + 'static,
    T::Transport: Send + 'static,
    S: NewHttpService<T::Transport> + Send + 'static,
    S::Future: Send + 'static,
    S::IntoService: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
{
    run_incoming(move || {
        let incoming = Incoming::bind_tcp(make_service, addr, tls)?;
        Ok(incoming)
    })
}

/// Start an HTTP server using an Unix domain socket listener.
///
/// This function internally uses the multi-threaded Tokio runtime with the default configuration.
///
/// This function is available only on Unix platform.
#[cfg(unix)]
pub fn run_unix<P, T, S>(path: P, tls: T, make_service: S) -> crate::Result<()>
where
    P: AsRef<std::path::Path>,
    T: MakeTlsTransport<crate::net::unix::AddrStream> + Send + 'static,
    T::Future: Send + 'static,
    T::Transport: Send + 'static,
    S: NewHttpService<T::Transport> + Send + 'static,
    S::Future: Send + 'static,
    S::IntoService: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
{
    run_incoming(move || {
        let incoming = Incoming::bind_unix(make_service, path, tls)?;
        Ok(incoming)
    })
}

fn run_incoming<F, S, T, C>(f: F) -> crate::Result<()>
where
    F: FnOnce() -> crate::Result<S>,
    S: StreamService<Response = (C, T)>,
    C: HttpService,
    T: AsyncRead + AsyncWrite,
    Server<S>: Spawn<tokio::runtime::Runtime>,
{
    let mut entered = tokio_executor::enter().expect("nested run_incoming");
    let mut runtime = tokio::runtime::Runtime::new()?;

    let stream_service = f()?;
    let server = Server::builder(stream_service).build();
    server.start(&mut runtime);

    entered
        .block_on(runtime.shutdown_on_idle())
        .expect("shutdown cannot error");
    Ok(())
}
