use {
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
};

fn main() -> failure::Fallible<()> {
    let tls_acceptor = tokio_tls::TlsAcceptor::from({
        let identity = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/..keys/identity.pfx"))?;
        let der = native_tls::Identity::from_pkcs12(&identity, "mypass")?;
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
