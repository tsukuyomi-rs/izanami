use {
    echo_service::Echo,
    http::Response,
    native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor},
    tokio_tls::TlsAcceptor,
};

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let der = std::fs::read("./private/identity.p12")?;
    let cert = Identity::from_pkcs12(&der, "mypass")?;
    let acceptor = NativeTlsAcceptor::builder(cert).build()?;
    let acceptor = TlsAcceptor::from(acceptor);

    izanami::Server::build() //
        .acceptor(acceptor)
        .serve(echo)
}
