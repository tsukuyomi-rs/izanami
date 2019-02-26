use {
    futures::prelude::*,
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::Server,
        service::{ext::ServiceExt, stream::StreamExt},
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
        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        builder.set_certificate(&cert)?;
        builder.set_private_key(&pkey)?;
        builder.check_private_key()?;

        const SERVER_PROTO: &[u8] = b"\x02h2\x08http/1.1";
        builder.set_alpn_protos(SERVER_PROTO)?;
        builder.set_alpn_select_callback(|_ssl, client_proto| {
            openssl::ssl::select_next_proto(SERVER_PROTO, client_proto)
                .ok_or_else(|| openssl::ssl::AlpnError::NOACK)
        });

        builder.build()
    };

    let incoming_service = AddrIncoming::bind("127.0.0.1:5000")? //
        .into_service()
        .with_adaptors()
        .and_then({
            let logger = logger.clone();
            move |stream| {
                let remote_addr = stream.remote_addr();
                slog::info!(logger, "got a connection"; "remote_addr" => %remote_addr);

                ssl_acceptor.accept_async(stream).map_err(Into::into)
            }
        })
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

                izanami::service::service_fn(move |_req| {
                    Response::builder()
                        .header("content-type", "text/plain")
                        .body("Hello")
                })
            };

            (stream, service)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
