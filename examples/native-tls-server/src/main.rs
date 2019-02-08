use {
    echo_service::Echo, //
    http::Response,
    izanami::{http::Http, server::Server, tls::native_tls::TlsAcceptor},
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

    let mut server = Server::default()?;

    let acceptor = TlsAcceptor::from_pkcs12(IDENTITY, "mypass")?;
    server.start(
        Http::bind("127.0.0.1:4000") //
            .serve_with(acceptor, move || echo.clone()),
    )?;

    server.run()
}
