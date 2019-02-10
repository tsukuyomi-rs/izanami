use {
    echo_service::Echo,
    http::Response,
    izanami::{tls::openssl::Ssl, Http, Server},
    openssl::{pkey::PKey, x509::X509},
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

    let cert = X509::from_pem(CERTIFICATE)?;
    let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
    let ssl = Ssl::single_cert(cert, pkey);

    server.spawn(
        Http::bind("127.0.0.1:4000") //
            .with_tls(ssl)
            .serve(echo)?,
    );

    server.run()
}
