use {
    crate::{
        client::Client,
        runtime::{CurrentThread, DefaultRuntime, Runtime},
        service::MakeTestService,
    },
    izanami_util::RemoteAddr,
};

/// A type that simulates an HTTP server without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S, Rt> {
    make_service: S,
    runtime: Rt,
    cookie_enabled: bool,
    remote_addr: RemoteAddr,
}

impl<S> Server<S, ()>
where
    S: MakeTestService,
{
    /// Creates a `Server` using the specified service factory.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(make_service: S) -> crate::Result<Server<S, DefaultRuntime>>
    where
        DefaultRuntime: Runtime<S>,
    {
        Ok(Server::new2(make_service, DefaultRuntime::new()?))
    }

    /// Creates a `Server` using the specified service factory,
    /// without some restrictions around thread safety.
    pub fn new_current_thread(make_service: S) -> crate::Result<Server<S, CurrentThread>>
    where
        CurrentThread: Runtime<S>,
    {
        Ok(Server::new2(make_service, CurrentThread::new()?))
    }
}

impl<S, Rt> Server<S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    fn new2(make_service: S, runtime: Rt) -> Self {
        Self {
            make_service,
            runtime,
            cookie_enabled: false,
            remote_addr: RemoteAddr::tcp(([127, 0, 0, 1], 12345).into()),
        }
    }

    /// Returns a pair of reference to the inner values.
    pub fn get_ref(&self) -> (&S, &Rt) {
        (&self.make_service, &self.runtime)
    }

    /// Returns a pair of mutable reference to the inner values.
    pub fn get_mut(&mut self) -> (&mut S, &mut Rt) {
        (&mut self.make_service, &mut self.runtime)
    }

    /// Returns a reference to the remote address associated with this server.
    pub fn remote_addr(&self) -> &RemoteAddr {
        &self.remote_addr
    }

    /// Returns a mutable reference to the remote address associated with this server.
    pub fn remote_addr_mut(&mut self) -> &mut RemoteAddr {
        &mut self.remote_addr
    }

    /// Sets whether to save the Cookie entries or not.
    ///
    /// By default, the Cookie saving is disabled.
    pub fn enable_cookies(&mut self, value: bool) {
        self.cookie_enabled = value;
    }

    pub(crate) fn cookie_enabled(&self) -> bool {
        self.cookie_enabled
    }

    /// Create a `Client` associated with this server.
    pub fn client(&mut self) -> crate::Result<Client<'_, S, Rt>> {
        let service = self.runtime.make_service(
            self.make_service
                .make_service(crate::service::TestContext::new()),
        )?;
        Ok(Client::new(self, service))
    }

    /// Waits for completing the background task spawned by the service.
    pub fn shutdown(self) {
        self.runtime.shutdown();
    }
}
