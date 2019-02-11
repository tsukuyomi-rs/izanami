use {
    futures::{Async, Future, Poll},
    std::marker::PhantomData,
    tokio::sync::oneshot,
};

pub fn default<T>(f: impl FnOnce(&mut System<'_>) -> crate::Result<T>) -> crate::Result<T> {
    let runtime = tokio::runtime::Runtime::new()?;
    with_fn(runtime, f)
}

pub fn current_thread<T>(
    f: impl FnOnce(&mut System<'_, tokio::runtime::current_thread::Runtime>) -> crate::Result<T>,
) -> crate::Result<T> {
    let runtime = tokio::runtime::current_thread::Runtime::new()?;
    with_fn(runtime, f)
}

fn with_fn<Rt, T>(
    mut runtime: Rt,
    f: impl FnOnce(&mut System<'_, Rt>) -> crate::Result<T>,
) -> crate::Result<T>
where
    Rt: Runtime,
{
    let ret = f(&mut System {
        runtime: &mut runtime,
    })?;
    runtime.shutdown_on_idle();
    Ok(ret)
}

/// A type that executes server tasks.
///
/// This type consists of a Tokio runtime that drives server tasks
/// and handles to control the spawned tasks.
#[derive(Debug)]
pub struct System<'rt, Rt: Runtime = tokio::runtime::Runtime> {
    runtime: &'rt mut Rt,
}

impl<'rt, Rt: Runtime> System<'rt, Rt> {
    /// Start a server task onto this server.
    pub fn spawn<T>(&mut self, task: T) -> Handle<'rt, Rt>
    where
        T: Task<Rt>,
    {
        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let (tx_complete, rx_complete) = oneshot::channel();

        task.spawn(
            self,
            TaskConfig {
                rx_shutdown: ShutdownSignal {
                    inner: Some(rx_shutdown),
                },
                tx_complete,
            },
        );

        Handle {
            tx_shutdown: Some(tx_shutdown),
            rx_complete,
            _marker: PhantomData,
        }
    }
}

impl<'a, Rt> std::ops::Deref for System<'a, Rt>
where
    Rt: Runtime,
{
    type Target = Rt;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.runtime
    }
}

impl<'a, Rt> std::ops::DerefMut for System<'a, Rt>
where
    Rt: Runtime,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.runtime
    }
}

/// The handle for managing a spawned task.
#[derive(Debug)]
pub struct Handle<'rt, Rt: Runtime> {
    tx_shutdown: Option<oneshot::Sender<()>>,
    rx_complete: oneshot::Receiver<crate::Result<()>>,
    _marker: PhantomData<fn(&mut System<'rt, Rt>)>,
}

impl<'rt, Rt> Handle<'rt, Rt>
where
    Rt: Runtime,
{
    /// Send a shutdown signal to the associated task.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.tx_shutdown.take() {
            let _ = tx.send(());
        }
    }

    /// Wait for completion of the associated task and returns its result.
    pub fn wait_complete(self, sys: &mut System<'rt, Rt>) -> crate::Result<()> {
        sys.block_on(
            self.rx_complete //
                .then(|result| result.expect("recv error")),
        )
    }
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

pub trait Task<Rt: Runtime> {
    fn spawn(self, sys: &mut System<'_, Rt>, config: TaskConfig);
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
