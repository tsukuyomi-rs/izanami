use {
    failure::format_err,
    futures::prelude::*,
    http::{Request, Response},
    izanami::{
        h1::H1, //
        h2::H2,
        http::Connection,
        net::tcp::AddrIncoming,
        server::Server,
        service::ext::ServiceExt,
    },
    std::{fs, io, sync::Arc},
    tokio_rustls::{
        rustls::{NoClientAuth, ServerConfig, Session},
        TlsAcceptor,
    },
};

type BoxedStdError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> failure::Fallible<()> {
    let rustls_acceptor = TlsAcceptor::from({
        let certs = {
            let mut reader = io::BufReader::new(fs::File::open("keys/server-crt.pem")?);
            tokio_rustls::rustls::internal::pemfile::certs(&mut reader)
                .map_err(|_| format_err!("failed to read certificate file"))?
        };

        let priv_key = {
            let mut reader = io::BufReader::new(fs::File::open("keys/server-key.pem")?);
            let rsa_keys = {
                tokio_rustls::rustls::internal::pemfile::rsa_private_keys(&mut reader)
                    .map_err(|_| format_err!("failed to read private key file as RSA"))?
            };
            rsa_keys
                .into_iter()
                .next()
                .ok_or_else(|| format_err!("invalid private key"))?
        };

        let mut config = ServerConfig::new(NoClientAuth::new());
        config.set_single_cert(certs, priv_key)?;

        Arc::new(config)
    });

    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_err_into::<BoxedStdError>()
            .service_and_then(move |stream| {
                // start TLS handshaking and returns encrypted transport
                // asynchronously.
                rustls_acceptor //
                    .accept(stream)
                    .map_err(Into::into)
            })
            .service_map(|stream| {
                let is_h2 = match stream.get_ref().1.get_alpn_protocol() {
                    Some(proto) if proto.starts_with(b"h2") => true,
                    _ => false,
                };

                let service = MyService;

                if is_h2 {
                    Either::Left(H2::new().serve(stream, service))
                } else {
                    Either::Right(H1::new().serve(stream, service))
                }
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}

struct MyService;

impl<ReqBd> izanami::service::Service<Request<ReqBd>> for MyService {
    type Response = Response<String>;
    type Error = io::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(().into())
    }

    fn call(&mut self, _: http::Request<ReqBd>) -> Self::Future {
        futures::future::ok(
            Response::builder()
                .header("content-type", "text/plain")
                .body("Hello".into())
                .unwrap(),
        )
    }
}

enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Connection for Either<L, R>
where
    L: Connection,
    R: Connection,
    L::Error: Into<BoxedStdError>,
    R::Error: Into<BoxedStdError>,
{
    type Error = BoxedStdError;

    fn poll_close(&mut self) -> Poll<(), Self::Error> {
        match self {
            Either::Left(l) => l.poll_close().map_err(Into::into),
            Either::Right(r) => r.poll_close().map_err(Into::into),
        }
    }

    fn graceful_shutdown(&mut self) {
        match self {
            Either::Left(l) => l.graceful_shutdown(),
            Either::Right(r) => r.graceful_shutdown(),
        }
    }
}
