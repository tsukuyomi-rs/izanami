use {
    echo_service::Echo,
    http::Response,
    izanami::{tls::rustls::Rustls, Http, System},
};

const CERTIFICATE: &[u8] = include_bytes!("../../../test/server-crt.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("../../../test/server-key.pem");

fn main() -> izanami::Result<()> {
    System::with_default(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        let rustls = Rustls::no_client_auth() //
            .single_cert(CERTIFICATE, PRIVATE_KEY)?;
        sys.spawn(
            Http::bind("127.0.0.1:4000") //
                .with_tls(rustls)
                .serve(echo)?,
        );

        Ok(())
    })
}
