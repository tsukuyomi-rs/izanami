use {
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
    openssl::{
        pkey::PKey,
        ssl::{SslAcceptor, SslMethod},
        x509::X509,
    },
    tokio_openssl::SslAcceptorExt,
};

fn main() -> failure::Fallible<()> {
    let logger = {
        use sloggers::{
            terminal::{Destination, TerminalLoggerBuilder},
            types::{Format, Severity},
            Build,
        };

        let mut builder = TerminalLoggerBuilder::new();
        builder.level(Severity::Debug);
        builder.format(Format::Full);
        builder.destination(Destination::Stderr);

        builder.build()?
    };

    let cert = {
        let certificate = std::fs::read("test/server-crt.pem")?;
        X509::from_pem(&certificate)?
    };

    let pkey = {
        let private_key = std::fs::read("test/server-key.pem")?;
        PKey::private_key_from_pem(&private_key)?
    };

    let ssl_acceptor = {
        const SERVER_PROTO: &[u8] = b"\x02h2\x08http/1.1";

        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&pkey)?;
        builder.check_private_key()?;

        builder.set_alpn_protos(SERVER_PROTO)?;
        builder.set_alpn_select_callback(|_ssl, client_proto| {
            openssl::ssl::select_next_proto(SERVER_PROTO, client_proto)
                .ok_or_else(|| openssl::ssl::AlpnError::NOACK)
        });

        builder.build()
    };

    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(izanami::service::unit())
        .fixed_service()
        .and_then(move |(_, stream, mut protocol)| {
            let remote_addr = stream.remote_addr();

            slog::info!(logger, "got a connection"; "remote_addr" => %remote_addr);
            let logger = logger.new(slog::o!("remote_addr" => remote_addr.to_string()));

            ssl_acceptor
                .accept_async(stream)
                .map(move |stream| {
                    let service = {
                        let ssl = stream.get_ref().ssl();

                        let servername = ssl
                            .servername_raw(openssl::ssl::NameType::HOST_NAME)
                            .map(|name| String::from_utf8_lossy(name).into_owned());
                        slog::info!(
                            logger,
                            "establish a SSL session";
                            "servername" => ?servername,
                        );

                        match ssl.selected_alpn_protocol() {
                            Some(b"h2") => {
                                protocol.http2_only(true);
                            }
                            Some(b"http/1.1") => {
                                protocol.http1_only(true);
                            }
                            _ => (),
                        }

                        service_fn_ok(move |_req| {
                            let _ = &remote_addr;
                            let _ = &servername;
                            Response::builder()
                                .header("content-type", "text/plain")
                                .body("Hello")
                                .unwrap()
                        })
                    };
                    (service, stream, protocol)
                })
                .map_err(Into::into)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
