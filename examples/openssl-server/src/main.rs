use {
    echo_service::Echo,
    http::Response,
    openssl::{
        pkey::PKey,
        rsa::Rsa,
        ssl::{AlpnError, SslAcceptor, SslMethod},
        x509::X509,
    },
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
    let pkey = PKey::from_rsa(Rsa::private_key_from_pem(PRIVATE_KEY)?)?;

    let acceptor = {
        let mut builder = SslAcceptor::mozilla_modern(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&pkey)?;
        builder.set_alpn_protos(b"\x02h2\x08http/1.1")?;
        builder.set_alpn_select_callback(|_, protos| {
            const H2: &[u8] = b"\x02h2";
            if protos.windows(3).any(|window| window == H2) {
                Ok(b"h2")
            } else {
                Err(AlpnError::NOACK)
            }
        });
        builder.build()
    };

    izanami::Server::bind("127.0.0.1:4000")? //
        .accept(acceptor)
        .start(echo)
}
