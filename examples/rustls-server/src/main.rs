use {
    echo_service::Echo,
    http::Response,
    izanami::{tls::rustls::Rustls, Http, Server},
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let mut server = Server::default()?;

    let rustls = Rustls::no_client_auth() //
        .single_cert(CERTIFICATE, PRIVATE_KEY)?;
    server.spawn(
        Http::bind("127.0.0.1:4000") //
            .with_tls(rustls)
            .serve(echo)?,
    );

    server.run()
}
