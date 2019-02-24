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
    tokio_openssl::SslAcceptorExt,
};

fn main() -> failure::Fallible<()> {
    let cert = {
        let certificate = std::fs::read("keys/server-crt.pem")?;
        X509::from_pem(&certificate)?
    };

    let pkey = {
        let private_key = std::fs::read("keys/server-key.pem")?;
        PKey::private_key_from_pem(&private_key)?
    };

    let ssl_acceptor = {
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&pkey)?;
        builder.check_private_key()?;
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
