use {
    echo_service::Echo, //
    http::Response,
    izanami::{http::Http, server::Server, tls::native_tls::NativeTls},
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let mut server = Server::default()?;

    let native_tls = NativeTls::from_pkcs12(IDENTITY, "mypass")?;
    server.spawn(
        Http::bind("127.0.0.1:4000") //
            .with_tls(native_tls)
            .serve(echo)?,
    );

    server.run()
}
