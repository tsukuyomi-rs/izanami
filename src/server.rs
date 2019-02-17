//! HTTP server.

use {
    crate::{
        drain::{Signal, Watch},
        error::BoxedStdError,
        net::{Bind, Listener},
        runtime::{Block, Runtime, Spawn},
        service::{
            imp::{HttpResponseImpl, ResponseBodyImpl},
            HttpConnection, HttpService, RequestBody,
        },
        tls::{NoTls, TlsConfig, TlsWrapper},
        util::MapAsyncExt,
    },
    futures::{future::Executor, Async, Future, IntoFuture, Poll},
    http::{Request, Response},
    izanami_util::net::{RemoteAddr, ServerName},
};

type SpawnFn<Rt, F> = dyn FnMut(&mut Rt, &F, &Watch) -> crate::Result<()> + Send + 'static;

#[derive(Debug, Copy, Clone)]
enum HttpVersions {
    Http1Only,
    Http2Only,
    Fallback,
}

/// An HTTP server running on a specific runtime.
#[allow(missing_debug_implementations)]
pub struct HttpServer<F, Rt> {
    service_factory: F,
    signal: Signal,
    watch: Watch,
    spawn_fns: Vec<Box<SpawnFn<Rt, F>>>,
    http_versions: HttpVersions,
    alpn_protocols: Option<Vec<String>>,
}

impl<F, S, Rt> HttpServer<F, Rt>
where
    F: Fn() -> S,
    S: HttpService,
    Rt: Runtime,
{
    /// Creates a `HttpServer` using the specified service factory.
    pub fn new(service_factory: F) -> Self {
        let (signal, watch) = crate::drain::channel();
        Self {
            service_factory,
            signal,
            watch,
            spawn_fns: vec![],
            http_versions: HttpVersions::Fallback,
            alpn_protocols: None,
        }
    }

    /// Sets whether to use HTTP/1 is required or not.
    ///
    /// By default, the server try HTTP/2 at first and switches to HTTP/1 if failed.
    pub fn http1_only(self) -> Self {
        Self {
            http_versions: HttpVersions::Http1Only,
            ..self
        }
    }

    /// Sets whether to use HTTP/2 is required or not.
    ///
    /// By default, the server try HTTP/2 at first and switches to HTTP/1 if failed.
    pub fn http2_only(self) -> Self {
        Self {
            http_versions: HttpVersions::Http2Only,
            ..self
        }
    }

    /// Specifies the list of protocols used by ALPN.
    pub fn alpn_protocols(self, protocols: Vec<String>) -> Self {
        Self {
            alpn_protocols: Some(protocols),
            ..self
        }
    }

    /// Associate the specified I/O with this server.
    pub fn bind<B>(self, bind: B) -> crate::Result<Self>
    where
        B: Bind,
        B::Listener: Send + 'static,
        HttpServerTask<S, B::Listener, NoTls>: Spawn<Rt>,
    {
        self.bind_tls(bind, NoTls::default())
    }

    /// Associate the specified I/O and SSL/TLS configuration with this server.
    pub fn bind_tls<B, T>(mut self, bind: B, tls: T) -> crate::Result<Self>
    where
        B: Bind,
        B::Listener: Send + 'static,
        T: TlsConfig<B::Conn>,
        T::Wrapper: Send + 'static,
        HttpServerTask<S, B::Listener, T::Wrapper>: Spawn<Rt>,
    {
        let mut listeners = bind.into_listeners()?;
        let http_versions = self.http_versions;

        let alpn_protocols = match (&self.alpn_protocols, http_versions) {
            (Some(protocols), _) => protocols.clone(),
            (None, HttpVersions::Http1Only) => vec!["http/1.1".into()],
            (None, HttpVersions::Http2Only) => vec!["h2".into()],
            (None, HttpVersions::Fallback) => vec!["h2".into(), "http/1.1".into()],
        };
        let tls_wrapper = tls.into_wrapper(alpn_protocols)?;

        self.spawn_fns
            .push(Box::new(move |rt, service_factory, watch| {
                for listener in listeners.drain(..) {
                    rt.spawn(HttpServerTask {
                        service: (*service_factory)(),
                        listener,
                        tls_wrapper: tls_wrapper.clone(),
                        watch: watch.clone(),
                        http_versions,
                    });
                }
                Ok(())
            }));

        Ok(self)
    }

    /// Start all tasks registered on this server onto the specified runtime.
    ///
    /// This method returns a `Handle` to notify a shutdown signal to the all
    /// of spawned tasks and track their completion.
    pub fn start(self, rt: &mut Rt) -> crate::Result<Handle> {
        for mut spawn_fn in self.spawn_fns {
            (spawn_fn)(rt, &self.service_factory, &self.watch)?;
        }
        Ok(Handle {
            signal: self.signal,
        })
    }

    /// Run this server onto the specified runtime.
    pub fn run(self, rt: &mut Rt) -> crate::Result<()>
    where
        Handle: Block<Rt>,
    {
        let _ = self.start(rt)?.block(rt);
        Ok(())
    }
}

#[derive(Debug)]
pub struct Handle {
    signal: Signal,
}

impl Handle {
    /// Send a shutdown signal to the background tasks.
    pub fn shutdown(self) -> impl Future<Item = (), Error = ()> {
        self.signal.drain()
    }
}

impl Future for Handle {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.signal.poll()
    }
}

#[allow(missing_debug_implementations)]
pub struct HttpServerTask<S, L, T> {
    service: S,
    listener: L,
    tls_wrapper: T,
    watch: Watch,
    http_versions: HttpVersions,
}

/// A helper macro for creating a server task.
macro_rules! spawn_inner {
    (<$S:ty> $self:expr, $runtime:expr) => {{
        let self_ = $self;
        let runtime = $runtime;

        let watch = self_.watch;
        let mut service = self_.service;
        let tls_wrapper = self_.tls_wrapper;
        let mut executor = runtime.executor();

        let mut protocol = hyper::server::conn::Http::new() //
            .with_executor(executor.clone());
        match self_.http_versions {
            HttpVersions::Http1Only => {
                protocol.http1_only(true);
            }
            HttpVersions::Http2Only => {
                protocol.http2_only(true);
            }
            _ => {}
        }

        let serve_connection_fn = move |io, remote_addr, watch: Watch| {
            let mk_conn_fut = service
                .connect()
                .map_err(|e| log::error!("HttpService::connect error: {}", e.into()));

            let mk_stream_fut = tls_wrapper
                .wrap(io)
                .map_err(|e| log::error!("tls error: {}", e.into()));

            let protocol = protocol.clone();
            (mk_conn_fut, mk_stream_fut) //
                .into_future()
                .and_then(move |(connection, (stream, server_name))| {
                    let service = InnerService::<$S> {
                        connection,
                        remote_addr,
                        server_name,
                    };
                    let conn = protocol.serve_connection(stream, service).with_upgrades();
                    watch
                        .watching(conn, |c, _| c.poll(), |c| c.graceful_shutdown())
                        .map_err(|e| log::error!("connection error: {}", e))
                })
        };

        let spawn_all = SpawnAll {
            listener: self_.listener,
            executor: runtime.executor(),
            serve_connection_fn,
            state: SpawnAllState::Running,
        };

        let watching = watch.watching(
            spawn_all,
            |s, watch| s.poll_watching(watch),
            |s| s.shutdown(),
        );

        runtime.spawn(watching);
    }};
}

impl<S, L, T> Spawn<tokio::runtime::Runtime> for HttpServerTask<S, L, T>
where
    S: HttpService + Send + 'static,
    S::Future: Send + 'static,
    S::Connection: Send + 'static,
    <S::Connection as HttpConnection>::Future: Send + 'static,
    L: Listener + Send + 'static,
    T: TlsWrapper<L::Conn> + Send + 'static,
    T::Wrapped: Send + 'static,
    T::Future: Send + 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut tokio::runtime::Runtime) {
        spawn_inner!(<S> self, rt)
    }
}

impl<S, L, T> Spawn<tokio::runtime::current_thread::Runtime> for HttpServerTask<S, L, T>
where
    S: HttpService + 'static,
    S::Connection: 'static,
    S::Future: 'static,
    L: Listener + 'static,
    T: TlsWrapper<L::Conn> + 'static,
    T::Wrapped: Send + 'static,
    T::Future: 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut tokio::runtime::current_thread::Runtime) {
        spawn_inner!(<S> self, rt)
    }
}

#[allow(unused)]
struct SpawnAll<L, E, F> {
    listener: L,
    executor: E,
    serve_connection_fn: F,
    state: SpawnAllState,
}

enum SpawnAllState {
    Running,
    Done,
}

impl<L, E, F> SpawnAll<L, E, F> {
    fn shutdown(&mut self) {
        self.state = SpawnAllState::Done;
    }
}

impl<L, E, F, Fut> SpawnAll<L, E, F>
where
    L: Listener,
    E: Executor<Fut>,
    F: FnMut(L::Conn, Option<RemoteAddr>, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()> + 'static,
{
    fn poll_watching(&mut self, watch: &Watch) -> Poll<(), ()> {
        loop {
            match self.state {
                SpawnAllState::Running => match self.listener.poll_incoming() {
                    Ok(Async::Ready((conn, addr))) => {
                        let serve_connection =
                            (self.serve_connection_fn)(conn, addr, watch.clone());
                        self.executor
                            .execute(serve_connection)
                            .unwrap_or_else(|_e| log::error!("executor error"));
                        continue;
                    }
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => {
                        log::error!("accept error: {}", e);
                        self.state = SpawnAllState::Done;
                        return Ok(Async::Ready(()));
                    }
                },
                SpawnAllState::Done => return Ok(Async::Ready(())),
            };
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerService<S: HttpService> {
    connection: S::Connection,
    remote_addr: Option<RemoteAddr>,
    server_name: Option<ServerName>,
}

impl<S> hyper::service::Service for InnerService<S>
where
    S: HttpService + 'static,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S>;
    type Error = BoxedStdError;
    type Future = InnerServiceFuture<S>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let mut request = request.map(RequestBody::from_hyp);
        if let Some(addr) = &self.remote_addr {
            request.extensions_mut().insert(addr.clone());
        }
        if let Some(sni) = &self.server_name {
            request.extensions_mut().insert(sni.clone());
        }

        InnerServiceFuture {
            inner: self.connection.respond(request),
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerServiceFuture<S: HttpService> {
    inner: <S::Connection as HttpConnection>::Future,
}

impl<S> Future for InnerServiceFuture<S>
where
    S: HttpService,
{
    type Item = Response<InnerBody<S>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|response| response.into_response().map(|inner| InnerBody { inner }))
            .map_err(Into::into)
    }
}

type AssocResponse<S> = <<S as HttpService>::Connection as HttpConnection>::Response;

#[allow(missing_debug_implementations)]
struct InnerBody<S: HttpService> {
    inner: <AssocResponse<S> as HttpResponseImpl>::Body,
}

impl<S> hyper::body::Payload for InnerBody<S>
where
    S: HttpService + 'static,
{
    type Data = <AssocResponse<S> as HttpResponseImpl>::Data;
    type Error = BoxedStdError;

    #[inline]
    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        ResponseBodyImpl::poll_data(&mut self.inner)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        ResponseBodyImpl::poll_trailers(&mut self.inner)
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        let hint = ResponseBodyImpl::size_hint(&self.inner);
        match (hint.lower(), hint.upper()) {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}
