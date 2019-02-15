use {
    echo_service::Echo,
    http::Response,
    izanami::HttpServer,
    openssl::{
        pkey::PKey,
        ssl::{SslAcceptor, SslMethod},
        x509::X509,
    },
    tokio::runtime::Runtime,
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

    let cert = X509::from_pem(CERTIFICATE)?;
    let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
    let ssl = {
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&pkey)?;
        builder.check_private_key()?;
        builder
    };

    let mut rt = Runtime::new()?;
    HttpServer::new(move || echo.clone()) //
        .bind_tls("127.0.0.1:4000", ssl)?
        .run(&mut rt)
}
