#![cfg(unix)]

use {
    super::sleep_on_errors::{Listener, SleepOnErrors},
    futures::Poll,
    std::{io, os::unix::net::SocketAddr, path::Path, time::Duration},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        net::{UnixListener, UnixStream},
    },
    tower_service::Service,
};

impl Listener for UnixListener {
    type Conn = (UnixStream, SocketAddr);
    fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
        UnixListener::poll_accept(self)
    }
}

/// An Unix domain socket stream connected to a remote endpoint.
#[derive(Debug)]
pub struct AddrStream {
    stream: UnixStream,
    remote_addr: SocketAddr,
}

impl AddrStream {
    pub fn get_ref(&self) -> &UnixStream {
        &self.stream
    }

    pub fn get_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// Returns the remote peer's address returned from `UnixListener::poll_accept`.
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.remote_addr
    }
}

impl io::Read for AddrStream {
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.stream.read(dst)
    }
}

impl io::Write for AddrStream {
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.stream.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl AsyncRead for AddrStream {}

impl AsyncWrite for AddrStream {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.stream)
    }
}

/// An Unix domain socket transport that allows to sleep on accept errors.
#[derive(Debug)]
pub struct AddrIncoming {
    listener: SleepOnErrors<UnixListener>,
    local_addr: SocketAddr,
}

impl AddrIncoming {
    pub fn new(listener: UnixListener) -> io::Result<Self> {
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener: SleepOnErrors::new(listener),
            local_addr,
        })
    }

    pub fn bind<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        Self::new(UnixListener::bind(path)?)
    }

    #[inline]
    pub fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
    }

    #[inline]
    pub fn set_sleep_on_errors(&mut self, duration: Option<Duration>) -> &mut Self {
        self.listener.set_sleep_on_errors(duration);
        self
    }
}

impl Service<()> for AddrIncoming {
    type Response = AddrStream;
    type Error = io::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.listener.poll_ready()
    }

    #[inline]
    fn call(&mut self, _: ()) -> Self::Future {
        let (stream, remote_addr) = self
            .listener
            .next_incoming()
            .expect("the connection is not ready");
        futures::future::ok(AddrStream {
            stream,
            remote_addr,
        })
    }
}
