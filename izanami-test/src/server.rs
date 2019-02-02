use crate::{
    runtime::{CurrentThread, DefaultRuntime, Runtime},
    service::MakeTestService,
};

/// A type that simulates an HTTP server without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S, Rt> {
    pub(crate) make_service: S,
    pub(crate) runtime: Rt,
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
        Ok(Server {
            make_service,
            runtime: DefaultRuntime::new()?,
        })
    }

    /// Creates a `Server` using the specified service factory,
    /// without some restrictions around thread safety.
    pub fn new_current_thread(make_service: S) -> crate::Result<Server<S, CurrentThread>>
    where
        CurrentThread: Runtime<S>,
    {
        Ok(Server {
            make_service,
            runtime: CurrentThread::new()?,
        })
    }
}

impl<S, Rt> Server<S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    /// Returns a pair of reference to the inner values.
    pub fn get_ref(&self) -> (&S, &Rt) {
        (&self.make_service, &self.runtime)
    }

    /// Returns a pair of mutable reference to the inner values.
    pub fn get_mut(&mut self) -> (&mut S, &mut Rt) {
        (&mut self.make_service, &mut self.runtime)
    }

    /// Create a `Client` associated with this server.
    pub fn client(&mut self) -> crate::client::Builder<'_, S, Rt> {
        crate::client::Builder::new(self)
    }

    /// Waits for completing the background task spawned by the service.
    pub fn shutdown(self) {
        self.runtime.shutdown();
    }
}
