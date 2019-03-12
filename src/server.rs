//! An HTTP server implementation powered by `hyper` and `tower-service`.

pub mod h1;
mod watch;

#[allow(dead_code)]
type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;

use {
    self::watch::{Signal, Watch},
    futures::{future::Executor, Async, Future, Poll},
    izanami_service::Service,
    tokio::executor::DefaultExecutor,
};

/// An asynchronous object that manages the connection to a remote peer.
pub trait Connection {
    /// The error type which will returned from this connection.
    type Error;

    /// Polls the completion of this connection.
    fn poll_complete(&mut self) -> Poll<(), Self::Error>;

    /// Notifies a shutdown signal to the connection.
    ///
    /// The connection may accept some requests for a while even after
    /// receiving this notification.
    fn graceful_shutdown(&mut self);
}

/// The factory of `Connection`.
pub trait MakeConnection {
    /// The connection produced by this factory.
    type Connection: Connection<Error = Self::Error>;

    /// The error type when producing a connection.
    type Error;

    ///　A `Future` that produces a value of `Connection`.
    type Future: Future<Item = Self::Connection, Error = Self::Error>;

    /// Polls the connection from client, and create a `Future` if available.
    fn make_connection(&mut self) -> Poll<Self::Future, Self::Error>;
}

impl<T> MakeConnection for T
where
    T: Service<()>,
    T::Response: Connection<Error = T::Error>,
{
    type Connection = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn make_connection(&mut self) -> Poll<Self::Future, Self::Error> {
        futures::try_ready!(self.poll_ready());
        Ok(Async::Ready(self.call(())))
    }
}

/// The builder for creating an instance of `Server`.
#[derive(Debug)]
pub struct Builder<T, Sig = futures::future::Empty<(), ()>, Sp = DefaultExecutor> {
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
    /// Specifies the spawner using for spawning the background tasks per connection.
    pub fn spawner<Sp>(self, spawner: Sp) -> Builder<T, Sig, Sp>
    where
        Sp: Executor<Background<T>>,
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
    Sp: Executor<Background<T>>,
{
    /// Creates an instance of `Server` using the current configuration.
    pub fn build(self) -> Server<T, Sig, Sp> {
        let (signal, watch) = watch::channel();
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
pub struct Server<T, Sig = futures::future::Empty<(), ()>, Sp = DefaultExecutor> {
    state: ServerState<T, Sig, Sp>,
}

impl<T> Server<T>
where
    T: MakeConnection,
{
    /// Creates a new `Server` with the specified `MakeConnection`.
    pub fn new(make_connection: T) -> Server<T>
    where
        DefaultExecutor: Executor<Background<T>>,
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
        watch: watch::Watch,
        spawner: Sp,
    },
    Done(watch::Draining),
}

impl<T, Sig, Sp> Future for Server<T, Sig, Sp>
where
    T: MakeConnection,
    Sig: Future<Item = ()>,
    Sp: Executor<Background<T>>,
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
                        let conn = futures::try_ready!(make_connection.make_connection());
                        let background = Background {
                            state: BackgroundState::Connecting(conn),
                            watch: watch.clone(),
                            signaled: false,
                        };
                        spawner
                            .execute(background)
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

/// The background task spawned by `Server`.
#[allow(missing_debug_implementations)]
pub struct Background<T: MakeConnection> {
    state: BackgroundState<T>,
    watch: Watch,
    signaled: bool,
}

#[allow(missing_debug_implementations)]
enum BackgroundState<T: MakeConnection> {
    Connecting(T::Future),
    Running(T::Connection),
    Closed,
}

impl<T> Background<T>
where
    T: MakeConnection,
{
    fn poll2(&mut self) -> Poll<(), T::Error> {
        loop {
            self.state = match self.state {
                BackgroundState::Connecting(ref mut future) => {
                    let conn = futures::try_ready!(future.poll());
                    BackgroundState::Running(conn)
                }
                BackgroundState::Running(ref mut conn) => {
                    if !self.signaled && self.watch.poll_signaled() {
                        self.signaled = true;
                        conn.graceful_shutdown();
                    }
                    futures::try_ready!(conn.poll_complete());
                    return Ok(Async::Ready(()));
                }
                BackgroundState::Closed => unreachable!("invalid state"),
            };
        }
    }
}

impl<T> Future for Background<T>
where
    T: MakeConnection,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.poll2() {
            Ok(Async::Ready(())) => (),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(_e) => log::error!("error during polling background"),
        }
        self.state = BackgroundState::Closed;
        Ok(Async::Ready(()))
    }
}
