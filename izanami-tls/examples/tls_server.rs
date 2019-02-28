use {
    futures::prelude::*,
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::{h1::H1Connection, Server},
        service::{ext::ServiceExt, stream::StreamExt},
    },
};

fn main() -> failure::Fallible<()> {
    let tls_acceptor = tokio_tls::TlsAcceptor::from({
        let identity = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/..keys/identity.pfx"))?;
        let der = native_tls::Identity::from_pkcs12(&identity, "mypass")?;
        native_tls::TlsAcceptor::builder(der).build()?
    });

    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")?
            .into_service()
            .with_adaptors()
            .and_then(move |stream| {
                tls_acceptor //
                    .accept(stream)
                    .map_err(Into::into)
            })
            .map(|stream| {
                H1Connection::build(stream) //
                    .finish(izanami::service::service_fn(|_req| {
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body("Hello")
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}
