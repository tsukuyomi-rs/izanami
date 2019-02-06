#![cfg(feature = "openssl")]

use {
    ::openssl::{
        pkey::{HasPrivate, PKeyRef},
        ssl::{
            AlpnError, //
            ErrorCode,
            HandshakeError,
            MidHandshakeSslStream,
            ShutdownResult,
            SslMethod,
            SslStream as RawSslStream,
        },
        x509::X509Ref,
    },
    futures::{Async, Poll},
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

#[derive(Debug, Default)]
pub struct SslAcceptorBuilder {
    alpn_protocols: Vec<Vec<u8>>,
    _priv: (),
}

impl SslAcceptorBuilder {
    pub fn alpn_protocols<T>(self, protos: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<Vec<u8>>,
    {
        Self {
            alpn_protocols: protos.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    pub fn build(
        self,
        certificate: &X509Ref,
        private_key: &PKeyRef<impl HasPrivate>,
    ) -> crate::Result<SslAcceptor> {
        let mut builder = openssl::ssl::SslAcceptor::mozilla_modern(SslMethod::tls())?;
        builder.set_certificate(certificate)?;
        builder.set_private_key(private_key)?;

        if !self.alpn_protocols.is_empty() {
            let mut protocols = Vec::new();
            for proto in &self.alpn_protocols {
                if !proto.is_ascii() {
                    return Err(failure::format_err!("ALPN protocol must be ASCII").into());
                }
                if proto.len() > 255 {
                    return Err(failure::format_err!(
                        "the length of ALPN protocol must be less than 256"
                    )
                    .into());
                }
                protocols.push(proto.len() as u8);
                protocols.extend_from_slice(&*proto);
            }
            builder.set_alpn_protos(&*protocols)?;

            let protocols = self.alpn_protocols;
            builder.set_alpn_select_callback(move |_, protos| {
                WireFormat { bytes: protos }
                    .find(|&proto| protocols.iter().any(|p| *p == proto))
                    .ok_or_else(|| AlpnError::NOACK)
            });
        }

        Ok(SslAcceptor {
            inner: builder.build(),
        })
    }
}

#[allow(missing_debug_implementations)]
struct WireFormat<'a> {
    bytes: &'a [u8],
}

impl<'a> Iterator for WireFormat<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self.bytes.split_at(1) {
            (b"", ..) => None,
            (len, remains) => {
                let len = usize::from(len[0]);
                if remains.len() < len {
                    return None;
                }
                let (item, bytes) = remains.split_at(len);
                self.bytes = bytes;
                Some(item)
            }
        }
    }
}

#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct SslAcceptor {
    inner: openssl::ssl::SslAcceptor,
}

impl SslAcceptor {
    pub fn new(
        certificate: &X509Ref,
        private_key: &PKeyRef<impl HasPrivate>,
    ) -> crate::Result<Self> {
        SslAcceptorBuilder::default().build(certificate, private_key)
    }

    pub fn builder() -> SslAcceptorBuilder {
        SslAcceptorBuilder::default()
    }

    pub fn get_ref(&self) -> &openssl::ssl::SslAcceptor {
        &self.inner
    }
}

impl From<openssl::ssl::SslAcceptor> for SslAcceptor {
    fn from(inner: openssl::ssl::SslAcceptor) -> Self {
        Self { inner }
    }
}

impl<T> super::Acceptor<T> for SslAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Accepted = SslStream<T>;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        SslStream {
            state: State::MidHandshake(MidHandshake::Start {
                acceptor: self.clone(),
                io,
            }),
        }
    }
}

#[allow(missing_debug_implementations)]
pub struct SslStream<S> {
    state: State<S>,
}

#[allow(missing_debug_implementations)]
enum State<S> {
    MidHandshake(MidHandshake<S>),
    Ready(RawSslStream<S>),
    Gone,
}

impl<S> SslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// Return whether the stream completes the handshake process
    /// and is available for use as transport.
    ///
    /// If this method returns `false`, the stream may progress
    /// the handshake process before reading/writing the data.
    pub fn is_ready(&self) -> bool {
        match self.state {
            State::Ready(..) => true,
            _ => false,
        }
    }

    /// Progress the internal state to complete the handshake process
    /// so that the stream can be used as I/O.
    pub fn poll_ready(&mut self) -> Poll<(), io::Error> {
        match self.with_handshake(|_| Ok(())) {
            Ok(()) => Ok(Async::Ready(())),
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    Ok(Async::NotReady)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn with_handshake<T>(
        &mut self,
        op: impl FnOnce(&mut RawSslStream<S>) -> io::Result<T>,
    ) -> io::Result<T> {
        loop {
            self.state = match &mut self.state {
                State::MidHandshake(m) => match m.try_handshake()? {
                    Some(io) => State::Ready(io),
                    None => panic!("cannot perform handshake twice"),
                },
                State::Ready(io) => return op(io),
                State::Gone => return Err(io::ErrorKind::Other.into()),
            };
        }
    }
}

impl<S> io::Read for SslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.with_handshake(|io| io.read(buf))
    }
}

impl<S> io::Write for SslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.with_handshake(|io| io.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.state {
            State::MidHandshake(m) => m.flush(),
            State::Ready(io) => io.flush(),
            State::Gone => Err(io::ErrorKind::Other.into()),
        }
    }
}

impl<S> AsyncRead for SslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        true
    }
}

impl<S> AsyncWrite for SslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match &mut self.state {
            State::MidHandshake(m) => {
                futures::try_ready!(m.shutdown());
                self.state = State::Gone;
                Ok(Async::Ready(()))
            }
            State::Ready(io) => shutdown_io(io),
            State::Gone => Ok(().into()),
        }
    }
}

fn shutdown_io<S>(io: &mut RawSslStream<S>) -> Poll<(), io::Error>
where
    S: AsyncRead + AsyncWrite,
{
    match io.shutdown() {
        Ok(ShutdownResult::Sent) | Ok(ShutdownResult::Received) => Ok(Async::Ready(())),
        Err(ref e) if e.code() == ErrorCode::ZERO_RETURN => Ok(Async::Ready(())),
        Err(ref e) if e.code() == ErrorCode::WANT_READ || e.code() == ErrorCode::WANT_WRITE => {
            Ok(Async::NotReady)
        }
        Err(err) => Err({
            err.into_io_error()
                .unwrap_or_else(|e| io::Error::new(io::ErrorKind::Other, e))
        }),
    }
}

#[allow(missing_debug_implementations)]
enum MidHandshake<T> {
    Start { acceptor: SslAcceptor, io: T },
    Handshake(MidHandshakeSslStream<T>),
    Done,
}

impl<T> MidHandshake<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn try_handshake(&mut self) -> io::Result<Option<RawSslStream<T>>> {
        match std::mem::replace(self, MidHandshake::Done) {
            MidHandshake::Start { acceptor, io } => match acceptor.inner.accept(io) {
                Ok(io) => Ok(Some(io)),
                Err(HandshakeError::WouldBlock(s)) => {
                    *self = MidHandshake::Handshake(s);
                    Err(io::ErrorKind::WouldBlock.into())
                }
                Err(_e) => Err(io::Error::new(io::ErrorKind::Other, "handshake error")),
            },
            MidHandshake::Handshake(s) => match s.handshake() {
                Ok(io) => Ok(Some(io)),
                Err(HandshakeError::WouldBlock(s)) => {
                    *self = MidHandshake::Handshake(s);
                    Err(io::ErrorKind::WouldBlock.into())
                }
                Err(_e) => Err(io::Error::new(io::ErrorKind::Other, "handshake error")),
            },
            MidHandshake::Done => Ok(None),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            MidHandshake::Start { io, .. } => io.flush(),
            MidHandshake::Handshake(s) => s.get_mut().flush(),
            MidHandshake::Done => Err(io::ErrorKind::Other.into()),
        }
    }

    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match self {
            MidHandshake::Start { io, .. } => io.shutdown(),
            MidHandshake::Handshake(s) => s.get_mut().shutdown(),
            MidHandshake::Done => Err(io::ErrorKind::Other.into()),
        }
    }
}
