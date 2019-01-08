//! A lightweight implementation of HTTP server for Web frameworks.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod error;
mod io;
pub mod rt;
pub mod test;

pub use crate::{
    error::{Error, Result},
    io::{Acceptor, Listener},
};

use {
    bytes::{Buf, Bytes},
    futures::{Future, Poll, Stream},
    http::{Request, Response},
    hyper::{
        body::{Body, Payload as _Payload},
        server::conn::Http,
    },
    izanami_service::{
        http::{BufStream, IntoBufStream, Upgradable},
        MakeServiceRef, Service,
    },
    std::{marker::PhantomData, net::SocketAddr, rc::Rc, sync::Arc},
};

type CritError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A struct that represents the stream of chunks from client.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl BufStream for RequestBody {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0.poll_data()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}

impl Upgradable for RequestBody {
    type Upgraded = hyper::upgrade::Upgraded;
    type Error = hyper::error::Error;
    type Future = hyper::upgrade::OnUpgrade;

    fn on_upgrade(self) -> Self::Future {
        self.0.on_upgrade()
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<S, L = SocketAddr, A = (), R = tokio::runtime::Runtime> {
    make_service: S,
    listener: L,
    acceptor: A,
    protocol: Http,
    runtime: Option<R>,
}

impl<S> Server<S> {
    /// Create a new `Server` with the specified `NewService` and default configuration.
    pub fn new(make_service: S) -> Self {
        Self {
            make_service,
            listener: ([127, 0, 0, 1], 4000).into(),
            acceptor: (),
            protocol: Http::new(),
            runtime: None,
        }
    }
}

impl<S, L, A, R> Server<S, L, A, R> {
    /// Sets the transport used by the server.
    ///
    /// By default, a TCP transport with the listener address `"127.0.0.1:4000"` is set.
    pub fn bind<L2>(self, listener: L2) -> Server<S, L2, A, R>
    where
        L2: Listener,
    {
        Server {
            make_service: self.make_service,
            listener,
            acceptor: self.acceptor,
            protocol: self.protocol,
            runtime: self.runtime,
        }
    }

    /// Sets the instance of `Acceptor` to the server.
    ///
    /// By default, the raw acceptor is set, which returns the incoming
    /// I/Os directly.
    pub fn acceptor<A2>(self, acceptor: A2) -> Server<S, L, A2, R>
    where
        L: Listener,
        A2: Acceptor<L::Conn>,
    {
        Server {
            make_service: self.make_service,
            listener: self.listener,
            acceptor,
            protocol: self.protocol,
            runtime: self.runtime,
        }
    }

    /// Sets the HTTP-level configuration to this server.
    ///
    /// Note that the executor will be overwritten by the launcher.
    pub fn protocol(self, protocol: Http) -> Self {
        Self { protocol, ..self }
    }

    /// Sets the instance of runtime to the specified `runtime`.
    pub fn runtime<R2>(self, runtime: R2) -> Server<S, L, A, R2> {
        Server {
            make_service: self.make_service,
            listener: self.listener,
            acceptor: self.acceptor,
            protocol: self.protocol,
            runtime: Some(runtime),
        }
    }

    /// Switches the runtime to be used to [`current_thread::Runtime`].
    ///
    /// [`current_thread::Runtime`]: https://docs.rs/tokio/0.1/tokio/runtime/current_thread/struct.Runtime.html
    pub fn current_thread(self) -> Server<S, L, A, tokio::runtime::current_thread::Runtime> {
        Server {
            make_service: self.make_service,
            listener: self.listener,
            acceptor: self.acceptor,
            protocol: self.protocol,
            runtime: None,
        }
    }
}

/// A macro for creating a server task from the specified components.
macro_rules! serve {
    (
        make_service: $make_service:expr,
        listener: $listener:expr,
        acceptor: $acceptor:expr,
        protocol: $protocol:expr,
        spawn: $spawn:expr,
    ) => {{
        let make_service = $make_service;
        let listener = $listener;
        let acceptor = $acceptor;
        let protocol = $protocol;
        let spawn = $spawn;

        let incoming = listener
            .listen()
            .map_err(|err| failure::Error::from_boxed_compat(err.into()))?;
        incoming
            .map_err(|e| log::error!("transport error: {}", e.into()))
            .for_each(move |io| {
                let accept = acceptor
                    .accept(io)
                    .map_err(|e| log::error!("acceptor error: {}", e.into()));

                let protocol = protocol.clone();
                let make_service = make_service.clone();
                let task = accept.and_then(move |io| {
                    let service = make_service
                        .make_service_ref(&io)
                        .map_err(|e| log::error!("make_service error: {}", e.into()));
                    service
                        .and_then(|service| {
                            ReadyService(Some(service), PhantomData)
                                .map_err(|e| log::error!("service error: {}", e.into()))
                        })
                        .and_then(move |service| {
                            protocol
                                .serve_connection(io, LiftedHttpService { service })
                                .with_upgrades()
                                .map_err(|e| log::error!("HTTP protocol error: {}", e))
                        })
                });
                spawn(task);
                Ok(())
            })
    }};
}

impl<S, T, A, Bd> Server<S, T, A, tokio::runtime::Runtime>
where
    S: MakeServiceRef<A::Conn, Request<RequestBody>, Response = Response<Bd>>
        + Send
        + Sync
        + 'static,
    S::Error: Into<crate::CritError>,
    S::MakeError: Into<crate::CritError>,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
    Bd: IntoBufStream,
    Bd::Stream: Send + 'static,
    T: Listener,
    T::Incoming: Send + 'static,
    A: Acceptor<T::Conn> + Send + 'static,
    A::Conn: Send + 'static,
    A::Error: Into<crate::CritError>,
    A::Accept: Send + 'static,
{
    pub fn run(self) -> crate::Result<()> {
        let mut runtime = match self.runtime {
            Some(rt) => rt,
            None => tokio::runtime::Runtime::new()?,
        };

        let serve = serve! {
            make_service: Arc::new(self.make_service),
            listener: self.listener,
            acceptor: self.acceptor,
            protocol: Arc::new(
                self.protocol.with_executor(tokio::executor::DefaultExecutor::current())
            ),
            spawn: |future| crate::rt::spawn(future),
        };

        runtime.spawn(serve);
        runtime.shutdown_on_idle().wait().unwrap();

        Ok(())
    }
}

impl<S, T, A, Bd> Server<S, T, A, tokio::runtime::current_thread::Runtime>
where
    S: MakeServiceRef<A::Conn, Request<RequestBody>, Response = Response<Bd>> + 'static,
    S::Error: Into<crate::CritError>,
    S::MakeError: Into<crate::CritError>,
    S::Future: 'static,
    S::Service: 'static,
    <S::Service as Service<Request<RequestBody>>>::Future: 'static,
    Bd: IntoBufStream,
    Bd::Stream: Send + 'static,
    T: Listener,
    T::Incoming: 'static,
    A: Acceptor<T::Conn> + 'static,
    A::Conn: Send + 'static,
    A::Error: Into<crate::CritError>,
    A::Accept: 'static,
{
    pub fn run(self) -> crate::Result<()> {
        let mut runtime = match self.runtime {
            Some(rt) => rt,
            None => tokio::runtime::current_thread::Runtime::new()?,
        };

        let serve = serve! {
            make_service: Rc::new(self.make_service),
            listener: self.listener,
            acceptor: self.acceptor,
            protocol: Rc::new(
                self.protocol.with_executor(tokio::runtime::current_thread::TaskExecutor::current())
            ),
            spawn: |future| tokio::runtime::current_thread::spawn(future),
        };

        let _ = runtime.block_on(serve);
        runtime.run()?;

        Ok(())
    }
}

#[allow(missing_debug_implementations)]
struct LiftedHttpService<S> {
    service: S,
}

impl<S, Bd> hyper::service::Service for LiftedHttpService<S>
where
    S: Service<Request<RequestBody>, Response = Response<Bd>>,
    S::Error: Into<crate::CritError>,
    Bd: IntoBufStream,
    Bd::Stream: Send + 'static,
{
    type ReqBody = Body;
    type ResBody = Body;
    type Error = S::Error;
    type Future = LiftedHttpServiceFuture<S::Future>;

    #[inline]
    fn call(&mut self, request: Request<Body>) -> Self::Future {
        LiftedHttpServiceFuture {
            inner: self.service.call(request.map(RequestBody)),
        }
    }
}

#[allow(missing_debug_implementations)]
struct LiftedHttpServiceFuture<Fut> {
    inner: Fut,
}

impl<Fut, Bd> Future for LiftedHttpServiceFuture<Fut>
where
    Fut: Future<Item = Response<Bd>>,
    Bd: IntoBufStream,
    Bd::Stream: Send + 'static,
{
    type Item = Response<Body>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll().map(|x| {
            x.map(|response| {
                response.map(|body| {
                    let mut body = body.into_buf_stream();
                    Body::wrap_stream(futures::stream::poll_fn(move || {
                        body.poll_buf()
                            .map(|x| x.map(|data_opt| data_opt.map(|data| data.collect::<Bytes>())))
                    }))
                })
            })
        })
    }
}

#[allow(missing_debug_implementations)]
struct ReadyService<S, Req>(Option<S>, PhantomData<fn(Req)>);

impl<S, Req> Future for ReadyService<S, Req>
where
    S: Service<Req>,
{
    type Item = S;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        futures::try_ready!(self
            .0
            .as_mut()
            .expect("the future has already been polled")
            .poll_ready());
        Ok(futures::Async::Ready(self.0.take().unwrap()))
    }
}
