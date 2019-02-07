use {
    echo_service::Echo, //
    http::Response,
    izanami::tls::native_tls::TlsAcceptor,
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let acceptor = TlsAcceptor::from_pkcs12(IDENTITY, "mypass")?;

    izanami::Server::bind("127.0.0.1:4000")? //
        .accept(acceptor)
        .start(echo)
}
