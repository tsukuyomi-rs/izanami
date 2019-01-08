use {
    echo_service::Echo,
    http::{Request, Response},
    openssl::ssl::{AlpnError, SslAcceptor, SslFiletype, SslMethod},
    regex::Regex,
};

fn index<Bd>(_: Request<Bd>, _: &Regex) -> Response<String> {
    Response::builder().body("Hello".into()).unwrap()
}

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", index)? //
        .build();

    let mut builder = SslAcceptor::mozilla_modern(SslMethod::tls())?;
    builder.set_certificate_file("./private/cert.pem", SslFiletype::PEM)?;
    builder.set_private_key_file("./private/key.pem", SslFiletype::PEM)?;
    builder.set_alpn_protos(b"\x02h2\x08http/1.1")?;
    builder.set_alpn_select_callback(|_, protos| {
        const H2: &[u8] = b"\x02h2";
        if protos.windows(3).any(|window| window == H2) {
            Ok(b"h2")
        } else {
            Err(AlpnError::NOACK)
        }
    });
    let acceptor = builder.build();

    izanami::Server::new(echo) //
        .acceptor(acceptor)
        .run()
}
