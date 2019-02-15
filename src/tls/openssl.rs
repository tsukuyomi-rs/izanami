#![cfg(feature = "use-openssl")]

use {
    super::{TlsConfig, TlsWrapper},
    futures::{Async, Future, Poll},
    izanami_util::http::SniHostname,
    openssl::ssl::{AlpnError, SslAcceptor, SslAcceptorBuilder},
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_openssl::{SslAcceptorExt, SslStream},
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
    type Error = io::Error;
    type Future = AcceptAsync<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Future {
        AcceptAsync {
            inner: self.accept_async(io),
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct AcceptAsync<T: AsyncRead + AsyncWrite> {
    inner: tokio_openssl::AcceptAsync<T>,
}

impl<T> Future for AcceptAsync<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = (SslStream<T>, SniHostname);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(conn)) => {
                let sni_hostname = conn
                    .get_ref()
                    .ssl()
                    .servername(openssl::ssl::NameType::HOST_NAME)
                    .into();
                Ok(Async::Ready((conn, sni_hostname)))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(_e) => Err(io::Error::new(
                io::ErrorKind::Other,
                "OpenSSL handshake error",
            )),
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
