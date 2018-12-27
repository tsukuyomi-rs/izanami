use {
    native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor},
    tokio_tls::TlsAcceptor,
};

fn main() -> izanami::Result<()> {
    let der = std::fs::read("./private/identity.p12")?;
    let cert = Identity::from_pkcs12(&der, "mypass")?;
    let acceptor = NativeTlsAcceptor::builder(cert).build()?;
    let acceptor = TlsAcceptor::from(acceptor);

    izanami::Server::new(echo_service::Echo::default()) //
        .acceptor(acceptor)
        .run()
}
