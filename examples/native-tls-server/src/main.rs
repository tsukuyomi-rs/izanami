use {
    echo_service::Echo,
    http::{Request, Response},
    native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor},
    regex::Regex,
    tokio_tls::TlsAcceptor,
};

fn index<Bd>(_: Request<Bd>, _: &Regex) -> Response<String> {
    Response::builder().body("Hello".into()).unwrap()
}

fn main() -> izanami::Result<()> {
    let echo = Echo::builder().add_route("/", index)?.build();

    let der = std::fs::read("./private/identity.p12")?;
    let cert = Identity::from_pkcs12(&der, "mypass")?;
    let acceptor = NativeTlsAcceptor::builder(cert).build()?;
    let acceptor = TlsAcceptor::from(acceptor);

    izanami::Server::new(echo) //
        .acceptor(acceptor)
        .run()
}
