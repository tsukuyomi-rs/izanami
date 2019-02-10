#![cfg(feature = "rustls")]

use {
    super::*,
    ::rustls::{NoClientAuth, NoKeyLog, ServerConfig, ServerSession, Session, Stream},
    failure::format_err,
    futures::Poll,
    std::{io, sync::Arc},
};

#[allow(missing_debug_implementations)]
pub struct Rustls {
    server_config: ServerConfig,
}

impl Rustls {
    pub fn no_client_auth() -> Self {
        Self {
            server_config: {
                let mut config = ServerConfig::new(NoClientAuth::new());
                config.key_log = Arc::new(NoKeyLog);
                config
            },
        }
    }

    /// Sets a single certificate chain and matching private key.
    pub fn single_cert(mut self, certificate: &[u8], private_key: &[u8]) -> crate::Result<Self> {
        let certs = {
            let mut reader = io::BufReader::new(io::Cursor::new(certificate));
            ::rustls::internal::pemfile::certs(&mut reader)
                .map_err(|_| format_err!("failed to read certificate file"))?
        };

        let priv_key = {
            let mut reader = io::BufReader::new(io::Cursor::new(private_key));
            let rsa_keys = {
                ::rustls::internal::pemfile::rsa_private_keys(&mut reader)
                    .map_err(|_| format_err!("failed to read private key file as RSA"))?
            };
            rsa_keys
                .into_iter()
                .next()
                .ok_or_else(|| format_err!("invalid private key"))?
        };

        self.server_config.set_single_cert(certs, priv_key)?;

        Ok(self)
    }
}

impl From<ServerConfig> for Rustls {
    fn from(server_config: ServerConfig) -> Self {
        Self { server_config }
    }
}

impl<T> Tls<T> for Rustls
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = RustlsStream<T>;
    type Wrapper = RustlsWrapper;

    #[inline]
    fn wrapper(&self, config: TlsConfig) -> crate::Result<Self::Wrapper> {
        Ok(RustlsWrapper {
            config: Arc::new({
                let mut server_config = self.server_config.clone();
                server_config.set_protocols(&config.alpn_protocols);
                server_config
            }),
        })
    }
}

#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct RustlsWrapper {
    config: Arc<ServerConfig>,
}

impl<T> TlsWrapper<T> for RustlsWrapper
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = RustlsStream<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Wrapped {
        RustlsStream {
            io,
            is_shutdown: false,
            session: ServerSession::new(&self.config),
        }
    }
}

#[derive(Debug)]
pub struct RustlsStream<S> {
    io: S,
    is_shutdown: bool,
    session: ServerSession,
}

impl<S> RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn stream(&mut self) -> Stream<'_, ServerSession, S> {
        Stream::new(&mut self.session, &mut self.io)
    }
}

impl<S> io::Read for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream().read(buf)
    }
}

impl<S> io::Write for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream().write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.stream().flush()?;
        self.io.flush()
    }
}

impl<S> AsyncRead for RustlsStream<S> where S: AsyncRead + AsyncWrite {}

impl<S> AsyncWrite for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        if self.session.is_handshaking() {
            return Ok(().into());
        }

        if !self.is_shutdown {
            self.session.send_close_notify();
            self.is_shutdown = true;
        }

        tokio_io::try_nb!(io::Write::flush(self));
        self.io.shutdown()
    }
}
