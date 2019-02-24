use {
    failure::format_err,
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
    std::{io, sync::Arc},
    tokio_rustls::{
        rustls::{NoClientAuth, ServerConfig},
        TlsAcceptor,
    },
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() -> failure::Fallible<()> {
    let rustls_acceptor = TlsAcceptor::from({
        let certs = {
            let mut reader = io::BufReader::new(io::Cursor::new(CERTIFICATE));
            tokio_rustls::rustls::internal::pemfile::certs(&mut reader)
                .map_err(|_| format_err!("failed to read certificate file"))?
        };

        let priv_key = {
            let mut reader = io::BufReader::new(io::Cursor::new(PRIVATE_KEY));
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

    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(service_fn_ok(|()| {
            service_fn_ok(move |_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
                    .unwrap()
            })
        }))
        .fixed_service()
        .and_then(move |(service, stream, protocol)| {
            rustls_acceptor
                .accept(stream)
                .map(|stream| (service, stream, protocol))
                .map_err(Into::into)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
