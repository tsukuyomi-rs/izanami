use {
    futures::prelude::*,
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::Server,
        service::{ext::ServiceExt, stream::StreamExt},
    },
};

fn main() -> failure::Fallible<()> {
    let tls_acceptor = tokio_tls::TlsAcceptor::from({
        let identity = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/..keys/identity.pfx"))?;
        let der = native_tls::Identity::from_pkcs12(&identity, "mypass")?;
        native_tls::TlsAcceptor::builder(der).build()?
    });

    let incoming_service = AddrIncoming::bind("127.0.0.1:5000")?
        .into_service()
        .with_adaptors()
        .and_then(move |stream| tls_acceptor.accept(stream).map_err(Into::into))
        .map(|stream| {
            let service = izanami::service::service_fn(|_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
            });
            (stream, service)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
