use {
    crate::{
        client::{ResponseData, SendResponseBody},
        error::BoxedStdError,
        service::{MakeTestService, TestService},
    },
    futures::Future,
    http::Response,
    std::panic::{resume_unwind, AssertUnwindSafe},
};

/// A trait that abstracts the runtime for executing asynchronous computations
/// generated by the specified `Service`.
pub trait Runtime<S>: RuntimeImpl<S>
where
    S: MakeTestService,
{
}

#[doc(hidden)]
pub trait RuntimeImpl<S>
where
    S: MakeTestService,
{
    fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service>;

    fn call(
        &mut self,
        future: <S::Service as TestService>::Future,
    ) -> crate::Result<Response<S::ResponseBody>>;

    fn send_response_body(
        &mut self,
        body: SendResponseBody<S::ResponseBody>,
    ) -> crate::Result<ResponseData>;

    fn shutdown(self);
}

/// An implementor of `Runtime<S>` using the default Tokio runtime.
#[derive(Debug)]
pub struct DefaultRuntime {
    runtime: tokio::runtime::Runtime,
}

impl DefaultRuntime {
    pub(crate) fn new() -> crate::Result<Self> {
        let mut builder = tokio::runtime::Builder::new();
        builder.core_threads(1);
        builder.blocking_threads(1);
        builder.name_prefix("izanami-test");

        Ok(Self {
            runtime: builder.build()?,
        })
    }

    fn block_on<F>(&mut self, mut future: F) -> crate::Result<F::Item>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Into<BoxedStdError>,
    {
        let future = futures::future::poll_fn(move || {
            future.poll().map_err(crate::Error::from_boxed_compat)
        });
        match self
            .runtime
            .block_on(AssertUnwindSafe(future).catch_unwind())
        {
            Ok(result) => result,
            Err(err) => resume_unwind(Box::new(err)),
        }
    }
}

impl<S> Runtime<S> for DefaultRuntime
where
    S: MakeTestService,
    S::Service: Send + 'static,
    <S::Service as TestService>::Future: Send + 'static,
    S::Future: Send + 'static,
    S::ResponseBody: Send + 'static,
{
}

impl<S> RuntimeImpl<S> for DefaultRuntime
where
    S: MakeTestService,
    S::Service: Send + 'static,
    <S::Service as TestService>::Future: Send + 'static,
    S::Future: Send + 'static,
    S::ResponseBody: Send + 'static,
{
    fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service> {
        self.block_on(future)
    }

    fn call(
        &mut self,
        future: <S::Service as TestService>::Future,
    ) -> crate::Result<Response<S::ResponseBody>> {
        self.block_on(future)
    }

    fn send_response_body(
        &mut self,
        body: SendResponseBody<S::ResponseBody>,
    ) -> crate::Result<ResponseData> {
        self.block_on(body)
    }

    fn shutdown(self) {
        self.runtime.shutdown_on_idle().wait().unwrap();
    }
}

/// An implementor of `Runtime<S>` using single threaded Tokio runtime.
#[derive(Debug)]
pub struct CurrentThread {
    runtime: tokio::runtime::current_thread::Runtime,
}

impl CurrentThread {
    pub(crate) fn new() -> crate::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::current_thread::Runtime::new()?,
        })
    }

    fn block_on<F>(&mut self, mut future: F) -> crate::Result<F::Item>
    where
        F: Future,
        F::Error: Into<BoxedStdError>,
    {
        self.runtime.block_on(futures::future::poll_fn(move || {
            future.poll().map_err(crate::Error::from_boxed_compat)
        }))
    }
}

impl<S> Runtime<S> for CurrentThread where S: MakeTestService {}

impl<S> RuntimeImpl<S> for CurrentThread
where
    S: MakeTestService,
{
    fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service> {
        self.block_on(future)
    }

    fn call(
        &mut self,
        future: <S::Service as TestService>::Future,
    ) -> crate::Result<Response<S::ResponseBody>> {
        self.block_on(future)
    }

    fn send_response_body(
        &mut self,
        body: SendResponseBody<S::ResponseBody>,
    ) -> crate::Result<ResponseData> {
        self.block_on(body)
    }

    fn shutdown(mut self) {
        self.runtime.run().unwrap();
    }
}
