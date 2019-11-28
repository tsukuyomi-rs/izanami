#![allow(clippy::type_complexity)]

use async_trait::async_trait;
use http::{Request, Response};
use izanami::{App, Events};
use izanami_hyper::Events as HyperEvents;
use regex::{Regex, RegexSet};

type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

struct Router {
    routes: Vec<(
        Regex,
        Box<dyn for<'a> App<HyperEvents<'a>, Error = BoxedError> + Send + Sync + 'static>,
    )>,
    re_set: RegexSet,
}

impl<'a> App<HyperEvents<'a>> for Router {
    type Error = BoxedError;

    fn call<'l1, 'async_trait>(
        &'l1 self,
        request: Request<HyperEvents<'a>>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Self::Error>> + Send + 'async_trait>,
    >
    where
        'l1: 'async_trait,
        HyperEvents<'a>: 'async_trait,
    {
        match self
            .re_set
            .matches(request.uri().path())
            .iter()
            .next()
            .and_then(|index| self.routes.get(index))
        {
            Some((_re, app)) => app.call(request),
            None => Box::pin(async move {
                let mut events = request.into_body();
                events
                    .start_send_response(Response::new(()), true)
                    .await
                    .map_err(Into::into)
            }),
        }
    }
}

#[derive(Default)]
struct RouterBuilder {
    routes: Vec<(
        Regex,
        Box<dyn for<'a> App<HyperEvents<'a>, Error = BoxedError> + Send + Sync + 'static>,
    )>,
}

impl RouterBuilder {
    fn add<T>(&mut self, pattern: &str, app: T) -> anyhow::Result<&mut Self>
    where
        T: for<'a> App<HyperEvents<'a>, Error = BoxedError> + Send + Sync + 'static,
    {
        let pattern = Regex::new(pattern)?;
        self.routes.push((pattern, Box::new(app)));
        Ok(self)
    }

    fn build(&mut self) -> anyhow::Result<Router> {
        let patterns = self.routes.iter().map(|(re, _)| re.as_str());
        let re_set = RegexSet::new(patterns)?;

        Ok(Router {
            routes: std::mem::replace(&mut self.routes, vec![]),
            re_set,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut router = RouterBuilder::default();
    router.add("/", Hello)?;
    let router = router.build()?;

    let server = izanami_hyper::Server::bind("127.0.0.1:4000").await?;
    server.serve(std::sync::Arc::new(router)).await?;
    Ok(())
}

struct Hello;

#[async_trait]
impl<E: Events> App<E> for Hello
where
    E: Send,
    E::Data: Send,
    &'static str: Into<E::Data>,
{
    type Error = BoxedError;

    async fn call(&self, request: Request<E>) -> Result<(), Self::Error>
    where
        E: 'async_trait,
    {
        let mut events = request.into_body();
        events
            .start_send_response(Response::new(()), false)
            .await
            .map_err(Into::into)?;
        events
            .send_data("Hello, world!\n".into(), true)
            .await
            .map_err(Into::into)?;
        Ok(())
    }
}
