use {
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() -> failure::Fallible<()> {
    let tls_acceptor = tokio_tls::TlsAcceptor::from({
        let der = native_tls::Identity::from_pkcs12(IDENTITY, "mypass")?;
        native_tls::TlsAcceptor::builder(der).build()?
    });

    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(service_fn_ok(|()| {
            service_fn_ok(|_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
                    .unwrap()
            })
        }))
        .fixed_service()
        .and_then(move |(service, stream, protocol)| {
            tls_acceptor
                .accept(stream)
                .map(move |stream| (service, stream, protocol))
                .map_err(Into::into)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
