use {
    echo_service::Echo,
    failure::format_err,
    http::Response,
    izanami::System,
    rustls::{NoClientAuth, ServerConfig},
    std::io,
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

        let rustls = {
            let certs = {
                let mut reader = io::BufReader::new(io::Cursor::new(CERTIFICATE));
                ::rustls::internal::pemfile::certs(&mut reader)
                    .map_err(|_| format_err!("failed to read certificate file"))?
            };

            let priv_key = {
                let mut reader = io::BufReader::new(io::Cursor::new(PRIVATE_KEY));
                let rsa_keys = {
                    ::rustls::internal::pemfile::rsa_private_keys(&mut reader)
                        .map_err(|_| format_err!("failed to read private key file as RSA"))?
                };
                rsa_keys
                    .into_iter()
                    .next()
                    .ok_or_else(|| format_err!("invalid private key"))?
            };

            let mut config = ServerConfig::new(NoClientAuth::new());
            config.set_single_cert(certs, priv_key)?;
            config
        };

        izanami::http::server(move || echo.clone()) //
            .bind_tls("127.0.0.1:4000", rustls)
            .start(sys);

        Ok(())
    })
}
