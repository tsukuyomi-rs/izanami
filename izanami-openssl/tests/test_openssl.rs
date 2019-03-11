#![allow(clippy::redundant_closure)]

#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

// #[cfg(feature = "openssl")]
// mod openssl {
//     use {
//         futures::{Future, Stream},
//         http::Request,
//         hyper::{
//             client::{
//                 connect::{Connect, Connected, Destination},
//                 Client,
//             },
//             Body,
//         },
//         izanami::server::Server,
//         openssl_crate::{
//             pkey::PKey,
//             ssl::{
//                 SslAcceptor, //
//                 SslConnector,
//                 SslMethod,
//                 SslVerifyMode,
//             },
//             x509::X509,
//         },
//         std::{io, net::SocketAddr},
//         tokio::{
//             net::TcpStream, //
//             runtime::current_thread::Runtime,
//             sync::oneshot,
//         },
//         tokio_openssl::{SslConnectorExt, SslStream},
//     };
//
//     const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
//     const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");
//
//     #[test]
//     fn tls_server() -> failure::Fallible<()> {
//         let mut rt = Runtime::new()?;
//
//         let cert = X509::from_pem(CERTIFICATE)?;
//         let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
//         let openssl = {
//             let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
//             builder.set_certificate(&cert)?;
//             builder.set_private_key(&pkey)?;
//             builder.check_private_key()?;
//             builder.build()
//         };
//
//         let (tx_shutdown, rx_shutdown) = oneshot::channel();
//         let server = Server::bind_tcp("127.0.0.1:0")?
//             .use_tls(openssl)
//             .serve(super::Echo::default())
//             .with_graceful_shutdown(rx_shutdown);
//         let local_addr = server.local_addr();
//         server.start(&mut rt);
//
//         let client = Client::builder() //
//             .build(TestConnect {
//                 local_addr,
//                 connector: {
//                     let cert = X509::from_pem(CERTIFICATE)?;
//                     let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
//                     let mut builder = SslConnector::builder(SslMethod::tls())?;
//                     builder.set_verify(SslVerifyMode::NONE);
//                     builder.set_certificate(&cert)?;
//                     builder.set_private_key(&pkey)?;
//                     builder.build()
//                 },
//             });
//
//         let response = rt.block_on(
//             client.request(
//                 Request::get("http://localhost/") //
//                     .body(Body::empty())?,
//             ),
//         )?;
//         assert_eq!(response.status(), 200);
//
//         let body = rt.block_on(
//             response
//                 .into_body() //
//                 .concat2(),
//         )?;
//         assert_eq!(body.into_bytes(), "hello");
//
//         drop(client);
//         let _ = tx_shutdown.send(());
//         rt.run().unwrap();
//         Ok(())
//     }
//
//     struct TestConnect {
//         local_addr: SocketAddr,
//         connector: SslConnector,
//     }
//
//     impl Connect for TestConnect {
//         type Transport = SslStream<TcpStream>;
//         type Error = io::Error;
//         type Future = Box<
//             dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
//         >;
//
//         fn connect(&self, _: Destination) -> Self::Future {
//             let connector = self.connector.clone();
//             Box::new(
//                 TcpStream::connect(&self.local_addr)
//                     .and_then(move |stream| {
//                         connector
//                             .connect_async("localhost", stream)
//                             .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
//                     }) //
//                     .map(|stream| (stream, Connected::new())),
//             )
//         }
//     }
// }