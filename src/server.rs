//! HTTP server.

use {
    crate::{
        drain::{Signal, Watch},
        net::{Bind, Listener},
        service::{HttpService, MakeContext, MakeHttpService, RequestBody, ResponseBody},
        system::{CurrentThread, DefaultRuntime, Runtime, Spawn, System},
        tls::{NoTls, TlsConfig, TlsWrapper},
    },
    futures::{Async, Future, IntoFuture, Poll},
    http::{Request, Response},
    izanami_util::http::{RemoteAddr, SniHostname},
};

type SpawnFn<Rt, F> =
    dyn for<'s> FnMut(&mut System<'s, Rt>, &F, &Watch) -> crate::Result<()> + 'static;

#[derive(Debug, Copy, Clone)]
enum HttpVersions {
    Http1Only,
    Http2Only,
    Fallback,
}

/// An HTTP server running on `System`.
#[allow(missing_debug_implementations)]
pub struct HttpServer<F, Rt>
where
    Rt: Runtime,
{
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
    S: MakeHttpService,
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
        B: Bind + 'static,
        HttpServerTask<S, B::Listener, NoTls>: Spawn<Rt>,
    {
        self.bind_tls(bind, NoTls::default())
    }

    /// Associate the specified I/O and SSL/TLS configuration with this server.
    pub fn bind_tls<B, T>(mut self, bind: B, tls: T) -> crate::Result<Self>
    where
        B: Bind + 'static,
        T: TlsConfig<B::Conn> + 'static,
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
            .push(Box::new(move |sys, service_factory, watch| {
                for listener in listeners.drain(..) {
                    sys.spawn(HttpServerTask {
                        listener,
                        tls_wrapper: tls_wrapper.clone(),
                        make_service: (*service_factory)(),
                        watch: watch.clone(),
                        http_versions,
                    });
                }
                Ok(())
            }));

        Ok(self)
    }

    /// Start all tasks registered on this server onto the specified system.
    ///
    /// This method returns a `Handle` to notify a shutdown signal to the all of
    /// spawned tasks and track their completion.
    pub fn start(self, sys: &mut System<'_, Rt>) -> crate::Result<Handle> {
        for mut spawn_fn in self.spawn_fns {
            (spawn_fn)(sys, &self.service_factory, &self.watch)?;
        }
        Ok(Handle {
            signal: self.signal,
        })
    }

    /// Run this server onto the specified `System`.
    pub fn run(self, sys: &mut System<'_, Rt>) -> crate::Result<()>
    where
        Handle: crate::system::BlockOn<Rt>,
    {
        let server = self.start(sys)?;
        let _ = sys.block_on(server);
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

trait TaskExecutor<F: Future<Item = (), Error = ()> + 'static> {
    fn spawn(&mut self, future: F);
}

impl<F> TaskExecutor<F> for tokio::runtime::TaskExecutor
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(&mut self, future: F) {
        tokio::executor::Executor::spawn(self, Box::new(future))
            .unwrap_or_else(|e| log::error!("failed to spawn a task: {}", e))
    }
}

impl<F> TaskExecutor<F> for tokio::runtime::current_thread::TaskExecutor
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(&mut self, future: F) {
        self.spawn_local(Box::new(future))
            .unwrap_or_else(|e| log::error!("failed to spawn a task: {}", e))
    }
}

#[allow(unused)]
struct SpawnAll<L, E, F> {
    state: SpawnAllState<L, E, F>,
    watch: Watch,
}

enum SpawnAllState<L, E, F> {
    Running {
        listener: L,
        executor: E,
        serve_connection_fn: F,
    },
    Done,
}

impl<L, E, F> SpawnAll<L, E, F> {
    fn shutdown(&mut self) {
        self.state = SpawnAllState::Done;
    }
}

impl<L, E, F, Fut> Future for SpawnAll<L, E, F>
where
    L: Listener,
    E: TaskExecutor<Fut>,
    F: FnMut(L::Conn, RemoteAddr, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()> + 'static,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.state {
                SpawnAllState::Running {
                    ref mut listener,
                    ref mut executor,
                    ref mut serve_connection_fn,
                } => {
                    let (conn, addr) = match listener.poll_incoming() {
                        Ok(Async::Ready((conn, addr))) => (conn, addr),
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(e) => {
                            log::error!("accept error: {}", e);
                            return Ok(Async::Ready(()));
                        }
                    };
                    let future = serve_connection_fn(conn, addr, self.watch.clone());
                    executor.spawn(future);
                    continue;
                }
                SpawnAllState::Done => return Ok(Async::Ready(())),
            };
        }
    }
}

#[allow(missing_debug_implementations)]
pub struct HttpServerTask<S, L, T> {
    make_service: S,
    listener: L,
    tls_wrapper: T,
    watch: Watch,
    http_versions: HttpVersions,
}

/// A helper macro for creating a server task.
macro_rules! spawn_inner {
    ($self:expr, $runtime:expr) => {{
        let self_ = $self;
        let runtime = $runtime;

        let watch = self_.watch;
        let mut make_service = self_.make_service;
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

        let serve_connection_fn = move |conn, remote_addr, watch: Watch| {
            let mk_svc_fut = make_service
                .make_service(&mut MakeContext::new(&remote_addr))
                .map_err(|e| log::error!("make service error: {}", e.into()));

            let mk_conn_fut = tls_wrapper
                .wrap(conn)
                .map_err(|e| log::error!("tls error: {}", e.into()));

            let protocol = protocol.clone();

            (mk_svc_fut, mk_conn_fut) //
                .into_future()
                .and_then(move |(service, (conn, sni_hostname))| {
                    watch
                        .watching(
                            protocol
                                .serve_connection(
                                    conn,
                                    IzanamiService {
                                        service,
                                        remote_addr,
                                        sni_hostname,
                                    },
                                )
                                .with_upgrades(),
                            |conn| conn.graceful_shutdown(),
                        )
                        .map_err(|e| log::error!("connection error: {}", e))
                })
        };

        let spawn_all = SpawnAll {
            watch: watch.clone(),
            state: SpawnAllState::Running {
                listener: self_.listener,
                executor: runtime.executor(),
                serve_connection_fn,
            },
        };

        runtime.spawn(watch.watching(spawn_all, |s| s.shutdown()));
    }};
}

impl<S, L, T> Spawn for HttpServerTask<S, L, T>
where
    S: MakeHttpService + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
    L: Listener + Send + 'static,
    T: TlsWrapper<L::Conn> + Send + 'static,
    T::Wrapped: Send + 'static,
    T::Future: Send + 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut DefaultRuntime) {
        spawn_inner!(self, rt)
    }
}

impl<S, L, T> Spawn<CurrentThread> for HttpServerTask<S, L, T>
where
    S: MakeHttpService + 'static,
    S::Service: 'static,
    S::Future: 'static,
    L: Listener + 'static,
    T: TlsWrapper<L::Conn> + 'static,
    T::Wrapped: Send + 'static,
    T::Future: 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut CurrentThread) {
        spawn_inner!(self, rt)
    }
}

#[allow(missing_debug_implementations)]
struct IzanamiService<S> {
    service: S,
    remote_addr: RemoteAddr,
    sni_hostname: SniHostname,
}

impl<S> hyper::service::Service for IzanamiService<S>
where
    S: HttpService,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S::ResponseBody>;
    type Error = S::Error;
    type Future = LiftedHttpServiceFuture<S::Future>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let mut request = request.map(RequestBody::from_hyp);
        request.extensions_mut().insert(self.remote_addr.clone());
        request.extensions_mut().insert(self.sni_hostname.clone());

        LiftedHttpServiceFuture(self.service.call(request))
    }
}

#[allow(missing_debug_implementations)]
struct LiftedHttpServiceFuture<Fut>(Fut);

impl<Fut, Bd> Future for LiftedHttpServiceFuture<Fut>
where
    Fut: Future<Item = Response<Bd>>,
    Bd: ResponseBody + Send + 'static,
{
    type Item = Response<InnerBody<Bd>>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0
            .poll()
            .map(|x| x.map(|response| response.map(InnerBody)))
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<Bd>(Bd);

impl<Bd> hyper::body::Payload for InnerBody<Bd>
where
    Bd: ResponseBody + Send + 'static,
{
    type Data = Bd::Item;
    type Error = crate::error::BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_buf().map_err(Into::into)
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers().map_err(Into::into)
    }

    fn content_length(&self) -> Option<u64> {
        let hint = self.0.size_hint();
        match (hint.lower(), hint.upper()) {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}
