use {
    futures::prelude::*,
    http::Response,
    izanami::{
        h1::H1Connection, //
        net::tcp::AddrIncoming,
        server::Server,
        service::ext::ServiceExt,
    },
};

fn main() -> failure::Fallible<()> {
    let tls_acceptor = tokio_tls::TlsAcceptor::from({
        let identity = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/..keys/identity.pfx"))?;
        let der = native_tls::Identity::from_pkcs12(&identity, "mypass")?;
        native_tls::TlsAcceptor::builder(der).build()?
    });

    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_err_into::<Box<dyn std::error::Error + Send + Sync>>()
            .service_and_then(move |stream| {
                tls_acceptor //
                    .accept(stream)
                    .map_err(Into::into)
            })
            .service_map(|stream| {
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
