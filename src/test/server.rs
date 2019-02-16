use {
    super::{client::Client, service::MakeTestService},
    futures::Future,
    izanami_util::net::RemoteAddr,
};

/// A type that simulates an HTTP server without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S: MakeTestService> {
    make_service: S,
    remote_addr: RemoteAddr,
}

impl<S> Server<S>
where
    S: MakeTestService,
{
    pub fn new(make_service: S) -> Self {
        Self {
            make_service,
            remote_addr: RemoteAddr::Tcp(([127, 0, 0, 1], 12345).into()),
        }
    }

    /// Returns a pair of reference to the inner values.
    pub fn get_ref(&self) -> &S {
        &self.make_service
    }

    /// Returns a pair of mutable reference to the inner values.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.make_service
    }

    /// Returns a reference to the remote address associated with this server.
    pub fn remote_addr(&self) -> &RemoteAddr {
        &self.remote_addr
    }

    /// Returns a mutable reference to the remote address associated with this server.
    pub fn remote_addr_mut(&mut self) -> &mut RemoteAddr {
        &mut self.remote_addr
    }

    /// Create a `Client` associated with this server.
    pub fn client(&mut self) -> impl Future<Item = Client<S>, Error = S::MakeError> {
        self.make_service
            .make_service(super::service::TestContext::new())
            .map(Client::new)
    }
}
