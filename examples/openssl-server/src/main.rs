use {
    echo_service::Echo,
    http::Response,
    izanami::server::Server,
    openssl::{
        pkey::PKey,
        ssl::{SslAcceptor, SslMethod},
        x509::X509,
    },
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })
        .unwrap() //
        .build();

    let cert = X509::from_pem(CERTIFICATE).unwrap();
    let pkey = PKey::private_key_from_pem(PRIVATE_KEY).unwrap();
    let ssl = {
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        builder.set_certificate(&cert).unwrap();
        builder.set_private_key(&pkey).unwrap();
        builder.check_private_key().unwrap();
        builder.build()
    };

    izanami::rt::run(
        Server::bind_tcp("127.0.0.1:4000", ssl) //
            .unwrap()
            .serve(echo),
    )
}
