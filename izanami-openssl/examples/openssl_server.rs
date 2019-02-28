use {
    futures::prelude::*,
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::{h1::H1Connection, Server},
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

    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .into_service()
            .with_adaptors()
            .and_then(move |stream| {
                let remote_addr = stream.remote_addr();
                let logger = logger.new(slog::o!("remote_addr" => remote_addr.to_string()));
                slog::info!(logger, "got a connection");

                ssl_acceptor
                    .accept_async(stream)
                    .map(move |stream| (stream, logger))
                    .map_err(Into::into)
            })
            .map(move |(stream, logger)| {
                let logger = {
                    let ssl = stream.get_ref().ssl();

                    let servername = ssl
                        .servername_raw(openssl::ssl::NameType::HOST_NAME)
                        .map(|name| String::from_utf8_lossy(name).into_owned());
                    let logger = logger.new(slog::o!("servername" => servername));
                    slog::info!(
                        logger,
                        "establish a SSL session";
                    );

                    logger
                };

                H1Connection::build(stream) //
                    .finish(izanami::service::service_fn(
                        move |req: http::Request<_>| {
                            slog::info!(logger, "got a request";
                                "method" => %req.method(),
                                "path" => %req.uri().path(),
                            );
                            Response::builder()
                                .header("content-type", "text/plain")
                                .body("Hello")
                        },
                    ))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);

    Ok(())
}
