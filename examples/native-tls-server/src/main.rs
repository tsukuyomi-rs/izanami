use {
    echo_service::Echo, //
    http::Response,
    native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor},
    tokio_tls::TlsAcceptor,
};

const IDENTITY: &[u8] = include_bytes!("../../../test/identity.pfx");

fn main() {
    let echo = Echo::builder()
        .add_route("/", |_| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })
        .unwrap()
        .build();

    let der = Identity::from_pkcs12(IDENTITY, "mypass").unwrap();
    let tls: TlsAcceptor = NativeTlsAcceptor::builder(der).build().unwrap().into();

    izanami::run_tcp("127.0.0.1:4000", tls, echo).unwrap()
}
