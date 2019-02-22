use {
    super::sleep_on_errors::{Listener, SleepOnErrors},
    futures::{Poll, Stream},
    izanami_util::*,
    std::{
        io,
        net::{SocketAddr, TcpListener as StdTcpListener, ToSocketAddrs},
        time::Duration,
    },
    tokio::{
        io::{AsyncRead, AsyncWrite},
        net::{TcpListener, TcpStream},
        reactor::Handle,
    },
};

impl Listener for TcpListener {
    type Conn = (TcpStream, SocketAddr);
    #[inline]
    fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
        self.poll_accept()
    }
}

/// A TCP stream connected to a remote endpoint.
#[derive(Debug)]
pub struct AddrStream {
    stream: TcpStream,
    remote_addr: SocketAddr,
}

impl AddrStream {
    pub fn get_ref(&self) -> &TcpStream {
        &self.stream
    }

    pub fn get_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    /// Returns the remote peer's address returned from `TcpListener::poll_accept`.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
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

/// A TCP transport that allows to sleep on accept errors.
#[derive(Debug)]
pub struct AddrIncoming {
    listener: SleepOnErrors<TcpListener>,
    local_addr: SocketAddr,
    tcp_keepalive_timeout: Option<Duration>,
    tcp_nodelay: bool,
}

impl AddrIncoming {
    pub fn new(listener: TcpListener) -> io::Result<Self> {
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener: SleepOnErrors::new(listener),
            local_addr,
            tcp_keepalive_timeout: None,
            tcp_nodelay: false,
        })
    }

    pub fn bind<A>(addr: A) -> io::Result<Self>
    where
        A: ToSocketAddrs,
    {
        let std_listener = StdTcpListener::bind(addr)?;
        let listener = TcpListener::from_std(std_listener, &Handle::default())?;
        Self::new(listener)
    }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[inline]
    pub fn set_keepalive_timeout(&mut self, timeout: Option<Duration>) -> &mut Self {
        self.tcp_keepalive_timeout = timeout;
        self
    }

    #[inline]
    pub fn set_nodelay(&mut self, enabled: bool) -> &mut Self {
        self.tcp_nodelay = enabled;
        self
    }

    #[inline]
    pub fn set_sleep_on_errors(&mut self, duration: Option<Duration>) -> &mut Self {
        self.listener.set_sleep_on_errors(duration);
        self
    }
}

impl Stream for AddrIncoming {
    type Item = AddrStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        self.listener
            .poll_accept() //
            .map_async(|(stream, remote_addr)| {
                if let Some(timeout) = self.tcp_keepalive_timeout {
                    if let Err(e) = stream.set_keepalive(Some(timeout)) {
                        log::trace!("error trying to set TCP keepalive: {}", e);
                    }
                }

                if let Err(e) = stream.set_nodelay(self.tcp_nodelay) {
                    log::trace!("error trying to set TCP nodelay: {}", e);
                }

                Some(AddrStream {
                    stream,
                    remote_addr,
                })
            })
    }
}
