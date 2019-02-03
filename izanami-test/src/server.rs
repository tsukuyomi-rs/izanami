use {
    crate::{
        client::Client,
        runtime::{Awaitable, Runtime},
        service::MakeTestService,
    },
    izanami_util::RemoteAddr,
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
            remote_addr: RemoteAddr::tcp(([127, 0, 0, 1], 12345).into()),
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
    pub fn client<Rt>(&mut self) -> impl Awaitable<Rt, Ok = Client<'_, S>, Error = crate::Error>
    where
        Rt: Runtime<S::Future>,
    {
        #[allow(missing_debug_implementations)]
        struct MakeClient<'s, S: MakeTestService> {
            future: S::Future,
            server: &'s mut Server<S>,
        }

        impl<'s, S, Rt> Awaitable<Rt> for MakeClient<'s, S>
        where
            S: MakeTestService,
            Rt: Runtime<S::Future>,
        {
            type Ok = Client<'s, S>;
            type Error = crate::Error;

            fn wait(self, rt: &mut Rt) -> Result<Self::Ok, Self::Error> {
                let service = rt.block_on(self.future)?;
                Ok(Client::new(self.server, service))
            }
        }

        MakeClient {
            future: self
                .make_service
                .make_service(crate::service::TestContext::new()),
            server: self,
        }
    }
}
