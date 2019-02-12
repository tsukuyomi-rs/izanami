#![cfg(feature = "openssl")]

use {
    super::{TlsConfig, TlsWrapper},
    futures::{Async, Poll},
    openssl::ssl::{
        AlpnError, //
        ErrorCode,
        HandshakeError,
        MidHandshakeSslStream,
        ShutdownResult,
        SslAcceptor,
        SslAcceptorBuilder,
        SslStream as RawSslStream,
    },
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

impl<T> TlsConfig<T> for SslAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = SslStream<T>;
    type Wrapper = Self;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(self)
    }
}

impl<T> TlsConfig<T> for SslAcceptorBuilder
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = SslStream<T>;
    type Wrapper = SslAcceptor;

    #[inline]
    fn into_wrapper(mut self, alpn_protocols: Vec<String>) -> crate::Result<Self::Wrapper> {
        if !alpn_protocols.is_empty() {
            let wired = wire_format::to_vec(&alpn_protocols)?;
            self.set_alpn_protos(&*wired)?;

            let server_protos = alpn_protocols;
            self.set_alpn_select_callback(move |_, client_protos| {
                select_alpn_proto(wire_format::iter(client_protos), &server_protos)
                    .ok_or_else(|| AlpnError::NOACK)
            });
        }

        Ok(self.build())
    }
}

fn select_alpn_proto<'c>(
    client_protos: impl IntoIterator<Item = &'c [u8]>,
    server_protos: &[String],
) -> Option<&'c [u8]> {
    client_protos
        .into_iter()
        .find(|&proto| server_protos.iter().any(|p| p.as_bytes() == proto))
}

#[test]
fn test_select_alpn_proto() {
    assert_eq!(
        select_alpn_proto(
            vec![b"h2".as_ref(), b"http/1.1".as_ref()],
            &["h2".into(), "http/1.1".into()]
        ),
        Some(b"h2".as_ref())
    );

    assert_eq!(
        select_alpn_proto(vec![b"h2".as_ref(), b"http/1.1".as_ref()], &["h2".into()]),
        Some(b"h2".as_ref())
    );

    assert_eq!(
        select_alpn_proto(
            vec![b"h2".as_ref(), b"http/1.1".as_ref()],
            &["http/1.1".into()]
        ),
        Some(b"http/1.1".as_ref())
    );

    assert_eq!(
        select_alpn_proto(
            vec![b"spdy/1".as_ref(), b"http/1.1".as_ref()],
            &["h2".into()]
        ),
        None
    );
}

impl<T> TlsWrapper<T> for SslAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = SslStream<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Wrapped {
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
            MidHandshake::Start { acceptor, io } => match acceptor.accept(io) {
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

mod wire_format {
    pub(super) fn to_vec<T>(protos: impl IntoIterator<Item = T>) -> crate::Result<Vec<u8>>
    where
        T: AsRef<str>,
    {
        let mut wired = Vec::new();
        for proto in protos {
            let proto = proto.as_ref();
            if !proto.is_ascii() {
                return Err(failure::format_err!("ALPN protocol must be ASCII").into());
            }
            if proto.len() > 255 {
                return Err(failure::format_err!(
                    "the length of ALPN protocol must be less than 256"
                )
                .into());
            }
            wired.push(proto.len() as u8);
            wired.extend_from_slice(proto.as_bytes());
        }

        Ok(wired)
    }

    pub(super) fn iter(bytes: &[u8]) -> Iter<'_> {
        Iter { bytes }
    }

    #[allow(missing_debug_implementations)]
    pub(super) struct Iter<'a> {
        bytes: &'a [u8],
    }

    impl<'a> Iterator for Iter<'a> {
        type Item = &'a [u8];

        fn next(&mut self) -> Option<Self::Item> {
            if self.bytes.is_empty() {
                return None;
            }
            match self.bytes.split_at(1) {
                (b"", ..) => None,
                (n, remains) => {
                    let len = usize::from(n[0]);
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

    #[test]
    fn iter_empty() {
        let input: &[u8] = b"";
        assert_eq!(iter(input).count(), 0);
    }

    #[test]
    fn iter_single() {
        let input: &[u8] = b"\x02h2";
        assert_eq!(iter(input).collect::<Vec<_>>(), vec![b"h2".as_ref()]);
    }

    #[test]
    fn iter_list() {
        let input: &[u8] = b"\x06spdy/1\x02h2\x08http/1.1";
        assert_eq!(
            iter(input).collect::<Vec<_>>(),
            vec![b"spdy/1".as_ref(), b"h2".as_ref(), b"http/1.1".as_ref()]
        );
    }
}
