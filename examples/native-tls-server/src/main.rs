use {
    echo_service::Echo, //
    http::Response,
    izanami::{tls::native_tls::NativeTls, Http, System},
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() -> izanami::Result<()> {
    System::with_default(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        let native_tls = NativeTls::from_pkcs12(IDENTITY, "mypass")?;
        sys.spawn(
            Http::bind("127.0.0.1:4000") //
                .with_tls(native_tls)
                .serve(echo)?,
        );

        Ok(())
    })
}
