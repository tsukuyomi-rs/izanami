use {
    echo_service::Echo,
    failure::{format_err, Fallible},
    http::{Request, Response},
    regex::Regex,
    rustls::{Certificate, KeyLogFile, NoClientAuth, PrivateKey, ServerConfig},
    std::{fs::File, io::BufReader, path::Path, sync::Arc},
};

fn index<Bd>(_: Request<Bd>, _: &Regex) -> Response<String> {
    Response::builder().body("Hello".into()).unwrap()
}

fn main() -> izanami::Result<()> {
    let echo = Echo::builder().add_route("/", index)?.build();

    let tls_acceptor = build_tls_acceptor()?;
    izanami::Server::new(echo) //
        .acceptor(tls_acceptor)
        .run()
}

fn build_tls_acceptor() -> Fallible<tokio_rustls::TlsAcceptor> {
    const CERTS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/private/cert.pem");
    const PRIV_KEY_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/private/key.pem");

    let client_auth = NoClientAuth::new();

    let mut config = ServerConfig::new(client_auth);
    config.key_log = Arc::new(KeyLogFile::new());

    let certs = load_certs(CERTS_PATH)?;
    let priv_key = load_private_key(PRIV_KEY_PATH)?;
    config.set_single_cert(certs, priv_key)?;

    config.set_protocols(&["h2".into(), "http/1.1".into()]);

    Ok(Arc::new(config).into())
}

fn load_certs(path: impl AsRef<Path>) -> Fallible<Vec<Certificate>> {
    let certfile = File::open(path)?;
    let mut reader = BufReader::new(certfile);
    rustls::internal::pemfile::certs(&mut reader)
        .map_err(|_| format_err!("failed to read certificate file"))
}

fn load_private_key(path: impl AsRef<Path>) -> Fallible<PrivateKey> {
    let rsa_keys = {
        let keyfile = File::open(&path)?;
        let mut reader = BufReader::new(keyfile);
        rustls::internal::pemfile::rsa_private_keys(&mut reader)
            .map_err(|_| format_err!("failed to read private key file as RSA"))?
    };

    let pkcs8_keys = {
        let keyfile = File::open(&path)?;
        let mut reader = BufReader::new(keyfile);
        rustls::internal::pemfile::pkcs8_private_keys(&mut reader)
            .map_err(|_| format_err!("failed to read private key file as PKCS8"))?
    };

    (pkcs8_keys.into_iter().next())
        .or_else(|| rsa_keys.into_iter().next())
        .ok_or_else(|| format_err!("invalid private key"))
}
