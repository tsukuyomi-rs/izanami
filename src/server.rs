use {
    futures::{Async, Future, Poll},
    tokio::sync::oneshot,
};

/// A type that executes server tasks.
///
/// This type consists of a Tokio runtime that drives server tasks
/// and handles to control the spawned tasks.
#[derive(Debug)]
pub struct Server<Rt = tokio::runtime::Runtime>
where
    Rt: Runtime,
{
    runtime: Rt,
    serve_handles: Vec<Handle>,
}

impl Server {
    /// Create a new `Server` using the default Tokio runtime.
    pub fn default() -> crate::Result<Server<tokio::runtime::Runtime>> {
        Ok(Server::new(tokio::runtime::Runtime::new()?))
    }

    /// Create a new `Server` using the single-threaded Tokio runtime.
    pub fn current_thread() -> crate::Result<Server<tokio::runtime::current_thread::Runtime>> {
        Ok(Server::new(tokio::runtime::current_thread::Runtime::new()?))
    }
}

impl<Rt> Server<Rt>
where
    Rt: Runtime,
{
    /// Create a new `Server` using the specified runtime.
    pub fn new(runtime: Rt) -> Self {
        Self {
            runtime,
            serve_handles: vec![],
        }
    }

    /// Start a server task onto this server.
    pub fn spawn<T>(&mut self, task: T)
    where
        T: Task<Rt>,
    {
        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let (tx_complete, rx_complete) = oneshot::channel();

        task.spawn(
            &mut self.runtime,
            TaskConfig {
                rx_shutdown: ShutdownSignal {
                    inner: Some(rx_shutdown),
                },
                tx_complete,
            },
        );

        self.serve_handles.push(Handle {
            tx_shutdown,
            rx_complete,
        });
    }

    #[doc(hidden)]
    pub fn runtime(&mut self) -> &mut Rt {
        &mut self.runtime
    }

    /// Send a shutdown signal to the background tasks.
    pub fn shutdown(&mut self) -> crate::Result<()> {
        let mut complete_signals = vec![];
        for handle in self.serve_handles.drain(..) {
            let _ = handle.tx_shutdown.send(());
            complete_signals.push(handle.rx_complete);
        }

        self.runtime.block_on(
            futures::future::join_all(complete_signals) //
                .then(|result| match result {
                    Ok(results) => {
                        for result in results {
                            result?;
                        }
                        Ok(())
                    }
                    Err(..) => Err(failure::format_err!("recv error").into()),
                }),
        )
    }

    pub fn run(self) -> crate::Result<()> {
        self.runtime.shutdown_on_idle();
        Ok(())
    }
}

/// The handle for managing a spawned server task.
#[derive(Debug)]
struct Handle {
    tx_shutdown: oneshot::Sender<()>,
    rx_complete: oneshot::Receiver<crate::Result<()>>,
}

/// A trait abstracting the runtime that executes asynchronous tasks.
pub trait Runtime {
    fn block_on<F>(&mut self, future: F) -> Result<F::Item, F::Error>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Send + 'static;

    fn shutdown_on_idle(self);
}

impl Runtime for tokio::runtime::Runtime {
    fn block_on<F>(&mut self, future: F) -> Result<F::Item, F::Error>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Send + 'static,
    {
        self.block_on(future)
    }

    fn shutdown_on_idle(self) {
        self.shutdown_on_idle().wait().unwrap();
    }
}

impl Runtime for tokio::runtime::current_thread::Runtime {
    fn block_on<F>(&mut self, future: F) -> Result<F::Item, F::Error>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Send + 'static,
    {
        self.block_on(future)
    }

    fn shutdown_on_idle(mut self) {
        self.run().unwrap();
    }
}

pub trait Task<Rt: ?Sized> {
    fn spawn(self, rt: &mut Rt, config: TaskConfig);
}

#[derive(Debug)]
pub struct TaskConfig {
    pub(crate) rx_shutdown: ShutdownSignal,
    pub(crate) tx_complete: oneshot::Sender<crate::Result<()>>,
}

#[derive(Debug)]
pub(crate) struct ShutdownSignal {
    inner: Option<oneshot::Receiver<()>>,
}

impl Future for ShutdownSignal {
    type Item = ();
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.inner.is_none() {
            return Ok(Async::NotReady);
        }
        match self.inner.as_mut().unwrap().poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(())) => {
                log::trace!("receive a shutdown signal from sender");
                Ok(Async::Ready(()))
            }
            Err(..) => {
                log::trace!("switch to infinity run");
                let _ = self.inner.take();
                Ok(Async::NotReady)
            }
        }
    }
}
