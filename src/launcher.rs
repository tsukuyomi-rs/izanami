use {
    crate::{
        app::{App, AppService},
        body::HttpBody,
        server::{
            net::tcp::AddrIncoming,
            protocol::{H1, H2},
            service::ServiceExt,
            MakeConnection, Server,
        },
    },
    futures::Future,
    std::{io, net::ToSocketAddrs},
    tokio::runtime::Runtime,
};

#[derive(Debug)]
enum Protocol {
    H1(H1),
    H2(H2),
}

/// Web application launcher.
#[derive(Debug)]
pub struct Launcher<T> {
    app: T,
    protocol: Protocol,
    runtime: Runtime,
}

impl<T> Launcher<T>
where
    T: App + Clone + Send + 'static,
    T::Body: Send + 'static,
    T::Handler: Send + 'static,
    <T::Body as HttpBody>::Data: Send + 'static,
    <T::Body as HttpBody>::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    pub fn new(app: T) -> io::Result<Self> {
        Ok(Launcher {
            app,
            protocol: Protocol::H1(H1::new()),
            runtime: Runtime::new()?,
        })
    }

    pub fn use_h1(&mut self, protocol: H1) {
        self.protocol = Protocol::H1(protocol);
    }

    pub fn use_h2(&mut self, protocol: H2) {
        self.protocol = Protocol::H2(protocol);
    }

    fn bind_connection<C>(&mut self, make_connection: C)
    where
        C: MakeConnection + Send + 'static,
        C::Connection: Send + 'static,
        C::Future: Send + 'static,
        C::MakeError: std::fmt::Display,
    {
        self.runtime
            .spawn(Server::new(make_connection).map_err(|e| eprintln!("server error: {}", e)));
    }

    pub fn bind<A>(&mut self, addr: A) -> io::Result<()>
    where
        A: ToSocketAddrs,
    {
        let app = self.app.clone();
        match self.protocol {
            Protocol::H1(ref proto) => {
                let proto = proto.clone();
                self.bind_connection(
                    AddrIncoming::bind(addr)? //
                        .service_map(move |stream| {
                            let service = AppService::from(app.clone());
                            proto.serve(stream, service)
                        }),
                );
            }
            Protocol::H2(ref proto) => {
                let proto = proto.clone();
                self.bind_connection(
                    AddrIncoming::bind(addr)? //
                        .service_map(move |stream| {
                            let service = AppService::from(app.clone());
                            proto.serve(stream, service)
                        }),
                );
            }
        }

        Ok(())
    }

    pub fn run_forever(self) {
        let mut entered = tokio_executor::enter()
            .expect("another executor has already been set on the current thread");
        let shutdown = self.runtime.shutdown_on_idle();

        entered.block_on(shutdown).expect("shutdown never fail");
    }
}
