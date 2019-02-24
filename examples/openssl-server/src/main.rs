use {
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
    openssl::{
        pkey::PKey,
        ssl::{SslAcceptor, SslMethod},
        x509::X509,
    },
    std::io,
    tokio_openssl::SslAcceptorExt,
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() -> io::Result<()> {
    let cert = X509::from_pem(CERTIFICATE).unwrap();
    let pkey = PKey::private_key_from_pem(PRIVATE_KEY).unwrap();
    let ssl_acceptor = {
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        builder.set_certificate(&cert).unwrap();
        builder.set_private_key(&pkey).unwrap();
        builder.check_private_key().unwrap();
        builder.build()
    };

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
            ssl_acceptor
                .accept_async(stream)
                .map(|stream| (service, stream, protocol))
                .map_err(Into::into)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
