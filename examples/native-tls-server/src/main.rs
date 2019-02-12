use {
    echo_service::Echo, //
    http::Response,
    native_tls::{Identity, TlsAcceptor},
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() -> izanami::Result<()> {
    izanami::system::run(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        let der = Identity::from_pkcs12(IDENTITY, "mypass")?;
        let native_tls = TlsAcceptor::builder(der).build()?;

        sys.spawn(
            izanami::http::server(move || echo.clone()) //
                .bind_tls("127.0.0.1:4000", native_tls),
        );

        Ok(())
    })
}
