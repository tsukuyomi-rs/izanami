use {
    crate::handler::{HandlerService, NewHandler},
    futures::Future,
    izanami_http::{HttpBody, HttpUpgrade},
    izanami_server::{net::tcp::AddrIncoming, protocol::H1, Server},
    izanami_service::ServiceExt,
    std::{io, net::ToSocketAddrs},
    tokio::runtime::Runtime,
};

#[derive(Debug)]
pub struct Launcher<H> {
    new_handler: H,
    runtime: Runtime,
}

impl<H> Launcher<H>
where
    H: NewHandler + Clone + Send + 'static,
    H::Body: Send + 'static,
    <H::Body as HttpBody>::Data: Send + 'static,
    <H::Body as HttpBody>::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    <H::Body as HttpUpgrade>::UpgradeError: Into<Box<dyn std::error::Error + Send + Sync>>,
    H::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    H::Handler: Send + 'static,
{
    pub fn new(new_handler: H) -> io::Result<Self> {
        Ok(Launcher {
            new_handler,
            runtime: Runtime::new()?,
        })
    }

    pub fn bind<A>(&mut self, addr: A) -> io::Result<()>
    where
        A: ToSocketAddrs,
    {
        let new_handler = self.new_handler.clone();
        self.runtime.spawn(
            Server::new(
                AddrIncoming::bind(addr)? //
                    .service_map(move |stream| {
                        let service = HandlerService(new_handler.clone());
                        H1::new().serve(stream, service)
                    }),
            )
            .map_err(|e| eprintln!("server error: {}", e)),
        );
        Ok(())
    }

    pub fn run_forever(self) {
        let mut entered = tokio_executor::enter()
            .expect("another executor has already been set on the current thread");
        let shutdown = self.runtime.shutdown_on_idle();

        entered.block_on(shutdown).expect("shutdown never fail");
    }
}
