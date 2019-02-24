#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

// #[cfg(feature = "native-tls")]
// mod native_tls {
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
//         native_tls_crate::{
//             Certificate, //
//             Identity,
//             TlsAcceptor as NativeTlsAcceptor,
//             TlsConnector,
//         },
//         std::{io, net::SocketAddr},
//         tokio::{
//             net::TcpStream, //
//             runtime::current_thread::Runtime,
//             sync::oneshot,
//         },
//         tokio_tls::{TlsAcceptor, TlsStream},
//     };
//
//     #[test]
//     fn tls_server() -> failure::Fallible<()> {
//         let mut rt = Runtime::new()?;
//
//         const IDENTITY: &[u8] = include_bytes!("../test/identity.pfx");
//         const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
//
//         let tls: TlsAcceptor = {
//             let der = Identity::from_pkcs12(IDENTITY, "mypass")?;
//             NativeTlsAcceptor::builder(der).build()?.into()
//         };
//
//         let (tx_shutdown, rx_shutdown) = oneshot::channel();
//         let server = Server::bind_tcp("127.0.0.1:0")?
//             .use_tls(tls)
//             .serve(super::Echo::default())
//             .with_graceful_shutdown(rx_shutdown);
//         let local_addr = server.local_addr();
//         server.start(&mut rt);
//
//         let client = Client::builder() //
//             .build(TestConnect {
//                 local_addr,
//                 connector: TlsConnector::builder()
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
//
