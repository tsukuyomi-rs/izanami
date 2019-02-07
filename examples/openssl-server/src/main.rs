use {
    echo_service::Echo,
    http::Response,
    izanami::tls::openssl::SslAcceptor,
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

    let acceptor = {
        let cert = X509::from_pem(CERTIFICATE)?;
        let pkey = PKey::from_rsa(Rsa::private_key_from_pem(PRIVATE_KEY)?)?;
        SslAcceptor::builder() //
            .alpn_protocols(vec!["h2", "http/1.1"])
            .build(&cert, &pkey)?
    };

    izanami::Server::bind("127.0.0.1:4000")? //
        .accept(acceptor)
        .launch(echo)?
        .run()
}
