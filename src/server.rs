use {futures::Future, std::fmt, tokio::sync::oneshot};

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
    handles: Vec<ServeHandle<Rt>>,
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
            handles: vec![],
        }
    }

    /// Start a server task onto this server.
    pub fn start<T>(&mut self, new_task: T) -> crate::Result<()>
    where
        T: NewTask<Rt> + 'static,
    {
        let (tx, rx) = oneshot::channel();
        new_task.new_task(
            &mut self.runtime,
            NewTaskConfig {
                shutdown_signal: rx,
            },
        )?;
        self.handles.push(ServeHandle {
            new_task: Box::new(new_task),
            shutdown_signal: tx,
        });
        Ok(())
    }

    #[doc(hidden)]
    pub fn runtime(&mut self) -> &mut Rt {
        &mut self.runtime
    }

    /// Send shutdown signal to the background server tasks.
    pub fn shutdown(&mut self) {
        for handle in self.handles.drain(..) {
            if let Err(()) = handle.shutdown_signal.send(()) {
                log::trace!("the background task has already finished.");
            }
        }
    }

    pub fn run(self) -> crate::Result<()> {
        self.runtime.shutdown_on_idle();
        Ok(())
    }
}

#[derive(Debug)]
pub struct NewTaskConfig {
    pub(crate) shutdown_signal: oneshot::Receiver<()>,
}

/// A trait representing the factory of tasks used within `Server`.
pub trait NewTask<Rt: ?Sized> {
    fn new_task(&self, rt: &mut Rt, config: NewTaskConfig) -> crate::Result<()>;
}

impl<F, Rt: ?Sized> NewTask<Rt> for F
where
    F: Fn(&mut Rt, NewTaskConfig) -> crate::Result<()>,
{
    fn new_task(&self, rt: &mut Rt, config: NewTaskConfig) -> crate::Result<()> {
        (*self)(rt, config)
    }
}

/// The handle for managing a spawned HTTP server task.
struct ServeHandle<Rt: Runtime> {
    #[allow(dead_code)]
    new_task: Box<dyn NewTask<Rt> + 'static>,
    shutdown_signal: oneshot::Sender<()>,
}

impl<Rt> fmt::Debug for ServeHandle<Rt>
where
    Rt: Runtime,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeHandle").finish()
    }
}

/// A trait abstracting the runtime that executes asynchronous tasks.
pub trait Runtime {
    fn shutdown_on_idle(self);
}

impl Runtime for tokio::runtime::Runtime {
    fn shutdown_on_idle(self) {
        self.shutdown_on_idle().wait().unwrap();
    }
}

impl Runtime for tokio::runtime::current_thread::Runtime {
    fn shutdown_on_idle(mut self) {
        self.run().unwrap();
    }
}
