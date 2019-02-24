#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

// #[cfg(feature = "rustls")]
// mod rustls {
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
//         native_tls_crate::Certificate,
//         rustls_crate::{NoClientAuth, ServerConfig},
//         std::{io, net::SocketAddr, sync::Arc},
//         tokio::{
//             net::TcpStream, //
//             runtime::current_thread::Runtime,
//             sync::oneshot,
//         },
//         tokio_rustls::TlsAcceptor,
//         tokio_tls::TlsStream,
//     };
//
//     #[test]
//     fn tls_server() -> failure::Fallible<()> {
//         let mut rt = Runtime::new()?;
//
//         const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
//         const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");
//
//         let rustls: TlsAcceptor = {
//             let certs = {
//                 let mut reader = io::BufReader::new(io::Cursor::new(CERTIFICATE));
//                 rustls_crate::internal::pemfile::certs(&mut reader)
//                     .map_err(|_| failure::format_err!("failed to read certificate file"))?
//             };
//
//             let priv_key = {
//                 let mut reader = io::BufReader::new(io::Cursor::new(PRIVATE_KEY));
//                 let rsa_keys = {
//                     rustls_crate::internal::pemfile::rsa_private_keys(&mut reader).map_err(
//                         |_| failure::format_err!("failed to read private key file as RSA"),
//                     )?
//                 };
//                 rsa_keys
//                     .into_iter()
//                     .next()
//                     .ok_or_else(|| failure::format_err!("invalid private key"))?
//             };
//
//             let mut config = ServerConfig::new(NoClientAuth::new());
//             config.set_single_cert(certs, priv_key)?;
//
//             Arc::new(config).into()
//         };
//
//         let (tx_shutdown, rx_shutdown) = oneshot::channel();
//         let server = Server::bind_tcp("127.0.0.1:0")?
//             .use_tls(rustls)
//             .serve(super::Echo::default())
//             .with_graceful_shutdown(rx_shutdown);
//         let local_addr = server.local_addr();
//         server.start(&mut rt);
//
//         // FIXME: use rustls
//         let client = Client::builder() //
//             .build(TestConnect {
//                 local_addr,
//                 connector: native_tls_crate::TlsConnector::builder()
//                     .add_root_certificate(Certificate::from_pem(CERTIFICATE)?)
//                     .build()?
//                     .into(),
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
//     // FIXME: use rustls
//     struct TestConnect {
//         local_addr: SocketAddr,
//         connector: tokio_tls::TlsConnector,
//     }
//
//     impl Connect for TestConnect {
//         type Transport = TlsStream<TcpStream>;
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
//                             .connect("localhost", stream)
//                             .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
//                     }) //
//                     .map(|stream| (stream, Connected::new())),
//             )
//         }
//     }
// }
