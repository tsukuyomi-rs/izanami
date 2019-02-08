use {
    echo_service::Echo,
    http::Response,
    izanami::{tls::openssl::SslAcceptor, Http, Server},
    openssl::{pkey::PKey, rsa::Rsa, x509::X509},
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })? //
        .build();

    let mut server = Server::default()?;

    let acceptor = {
        let cert = X509::from_pem(CERTIFICATE)?;
        let pkey = PKey::from_rsa(Rsa::private_key_from_pem(PRIVATE_KEY)?)?;
        SslAcceptor::builder() //
            .alpn_protocols(vec!["h2", "http/1.1"])
            .build(&cert, &pkey)?
    };
    server.start(
        Http::bind("127.0.0.1:4000") //
            .serve_with(acceptor, move || echo.clone()),
    )?;

    server.run()
}
