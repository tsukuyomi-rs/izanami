//! Abstraction around HTTP services.

use {
    crate::drain::{Signal, Watch},
    futures::{future::Executor, Async, Future, Poll},
    izanami_service::Service,
};

pub trait Connection {
    type Future: Future<Item = (), Error = ()>;

    fn into_future(self, watch: Watch) -> Self::Future;
}

/// A trait abstracting a factory that produces the connection with a client
/// and the service associated with its connection.
pub trait MakeConnection {
    type Connection: Connection;

    /// The error type that will be thrown when acquiring a connection.
    type Error;

    ///　A `Future` that establishes the connection to client and initializes the service.
    type Future: Future<Item = Self::Connection, Error = Self::Error>;

    /// Polls the connection from client, and create a future that establishes
    /// its connection asynchronously.
    fn make_connection(&mut self) -> Poll<Self::Future, Self::Error>;
}

impl<T> MakeConnection for T
where
    T: Service<()>,
    T::Response: Connection,
{
    type Connection = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn make_connection(&mut self) -> Poll<Self::Future, Self::Error> {
        futures::try_ready!(self.poll_ready());
        Ok(Async::Ready(self.call(())))
    }
}

#[derive(Debug)]
pub struct Builder<T, Sig = futures::future::Empty<(), ()>, Sp = tokio::executor::DefaultExecutor> {
    make_connection: T,
    shutdown_signal: Sig,
    spawner: Sp,
}

impl<T> Builder<T>
where
    T: MakeConnection,
{
    /// Specifies the signal to shutdown the background tasks gracefully.
    pub fn with_graceful_shutdown<Sig>(self, signal: Sig) -> Builder<T, Sig>
    where
        Sig: Future<Item = ()>,
    {
        Builder {
            make_connection: self.make_connection,
            shutdown_signal: signal,
            spawner: self.spawner,
        }
    }
}

impl<T, Sig> Builder<T, Sig>
where
    T: MakeConnection,
    Sig: Future<Item = ()>,
{
    pub fn spawner<Sp>(self, spawner: Sp) -> Builder<T, Sig, Sp>
    where
        Sp: Executor<Conn<T>>,
    {
        Builder {
            make_connection: self.make_connection,
            shutdown_signal: self.shutdown_signal,
            spawner,
        }
    }
}

impl<T, Sig, Sp> Builder<T, Sig, Sp>
where
    T: MakeConnection,
    Sig: Future<Item = ()>,
    Sp: Executor<Conn<T>>,
{
    pub fn build(self) -> Server<T, Sig, Sp> {
        let (signal, watch) = crate::drain::channel();
        Server {
            state: ServerState::Running {
                make_connection: self.make_connection,
                shutdown_signal: self.shutdown_signal,
                signal: Some(signal),
                watch,
                spawner: self.spawner,
            },
        }
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<T, Sig = futures::future::Empty<(), ()>, Sp = tokio::executor::DefaultExecutor> {
    state: ServerState<T, Sig, Sp>,
}

impl<T> Server<T>
where
    T: MakeConnection,
{
    /// Creates a new `Server` with the specified `MakeConnection`.
    pub fn new(make_connection: T) -> Server<T>
    where
        tokio::executor::DefaultExecutor: Executor<Conn<T>>,
    {
        Server::builder(make_connection).build()
    }

    /// Creates a `Builder` with the specified `MakeConnection`, and starts building an instance of this type.
    pub fn builder(make_connection: T) -> Builder<T> {
        Builder {
            make_connection,
            shutdown_signal: futures::future::empty(),
            spawner: tokio::executor::DefaultExecutor::current(),
        }
    }
}

#[derive(Debug)]
enum ServerState<T, Sig, Sp> {
    Running {
        make_connection: T,
        shutdown_signal: Sig,
        signal: Option<Signal>,
        watch: Watch,
        spawner: Sp,
    },
    Done(crate::drain::Draining),
}

impl<T, Sig, Sp> Future for Server<T, Sig, Sp>
where
    T: MakeConnection,
    Sig: Future<Item = ()>,
    Sp: Executor<Conn<T>>,
{
    type Item = ();
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                ServerState::Running {
                    ref mut make_connection,
                    ref mut shutdown_signal,
                    ref mut signal,
                    ref watch,
                    ref spawner,
                } => match shutdown_signal.poll() {
                    Ok(Async::Ready(())) | Err(..) => {
                        let signal = signal.take().expect("unexpected condition");
                        ServerState::Done(signal.drain())
                    }
                    Ok(Async::NotReady) => {
                        let future = futures::try_ready!(make_connection.make_connection());
                        spawner
                            .execute(Conn {
                                state: ConnState::First(future, Some(watch.clone())),
                            })
                            .unwrap_or_else(|_e| log::error!("executor error"));
                        continue;
                    }
                },
                ServerState::Done(ref mut draining) => {
                    return draining
                        .poll()
                        .map_err(|_e| unreachable!("draining never fails"));
                }
            }
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct Conn<T: MakeConnection> {
    state: ConnState<T>,
}

enum ConnState<T: MakeConnection> {
    First(T::Future, Option<Watch>),
    Second(<T::Connection as Connection>::Future),
}

impl<T> Future for Conn<T>
where
    T: MakeConnection,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                ConnState::First(ref mut future, ref mut watch) => {
                    let conn = futures::try_ready!(future.poll().map_err(|_e| {
                        log::error!("make_connection error");
                    }));
                    let watch = watch.take().expect("should be available at here");
                    ConnState::Second(conn.into_future(watch))
                }
                ConnState::Second(ref mut future) => return future.poll(),
            };
        }
    }
}
