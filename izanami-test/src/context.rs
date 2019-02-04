use {
    crate::{error::BoxedStdError, runtime::Runtime},
    futures::Future,
    std::time::Duration,
};

#[derive(Debug)]
pub struct Context<'a, Rt: ?Sized = tokio::runtime::Runtime> {
    runtime: &'a mut Rt,
    timeout: Option<Duration>,
}

impl<'a, Rt: ?Sized> Context<'a, Rt> {
    pub(crate) fn new(runtime: &'a mut Rt) -> Self {
        Self {
            runtime,
            timeout: None,
        }
    }

    pub fn reborrow(&mut self) -> Context<'_, Rt> {
        Context {
            runtime: &mut *self.runtime,
            timeout: self.timeout,
        }
    }

    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn block_on<F>(&mut self, future: F) -> crate::Result<F::Item>
    where
        F: Future,
        F::Error: Into<BoxedStdError>,
        Rt: Runtime<F>,
    {
        self.runtime.block_on(future, self.timeout)
    }
}
