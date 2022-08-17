use std::{
    fmt, io,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{future::BoxFuture, FutureExt};
use http::{
    uri::{self, PathAndQuery},
    Request, Response, StatusCode, Uri,
};
use hyper::body::{Bytes, HttpBody};
use pin_project::pin_project;
use tower::Service;
use tower_http::services::{fs::DefaultServeDirFallback, ServeDir};

#[derive(Debug)]
pub enum Error {
    Hyper(hyper::Error),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Self::Hyper(e)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Hyper(e) => Some(e),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Hyper(e) => fmt::Display::fmt(e, f),
        }
    }
}

#[derive(Clone)]
pub struct PublicOr<I> {
    public: ServeDir<DefaultServeDirFallback>,
    inner: I,
}

impl<I> PublicOr<I> {
    pub fn new<P: AsRef<Path>>(inner: I, path: P) -> Self {
        Self {
            public: ServeDir::new(path),
            inner,
        }
    }
}

impl<I, ReqBody, IResBody> Service<Request<ReqBody>> for PublicOr<I>
where
    I: Service<Request<ReqBody>, Response = Response<IResBody>> + Send + Clone + 'static,
    I::Error: Into<io::Error>,
    I::Future: Send,
    ReqBody: Send + 'static,
    IResBody: http_body::Body<Data = Bytes> + Send + 'static,
    IResBody::Error: std::error::Error + Send + Sync + 'static,
{
    type Response = Response<super::ResponseBody>;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match Service::<Request<ReqBody>>::poll_ready(&mut self.public, cx) {
            Poll::Ready(Ok(())) => match self.inner.poll_ready(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
                Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            },
            Poll::Ready(Err(i)) => Poll::Ready(Err(i)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let mut fake_request = Request::new(hyper::body::Body::empty());
        *fake_request.method_mut() = parts.method.clone();
        *fake_request.headers_mut() = parts.headers.clone();
        *fake_request.uri_mut() = parts.uri.clone();

        let input = Request::from_parts(parts, body);

        let new_inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, new_inner);

        let fut = self.public.call(fake_request);
        async move {
            match fut.await {
                Ok(response) => {
                    if response.status() == StatusCode::NOT_FOUND {
                        inner
                            .call(input)
                            .await
                            .map(super::response_to_boxed_error_io)
                            .map_err(I::Error::into)
                    } else {
                        Ok(response.map(HttpBody::boxed_unsync))
                    }
                }
                Err(e) => Err(e),
            }
        }
        .boxed()
    }
}

#[pin_project(project = BaseRouterResponseBodyProj)]
pub enum BaseRouterResponseBody<A, P, S> {
    Api(#[pin] A),
    App(#[pin] P),
    Assets(#[pin] S),
    Other(#[pin] hyper::Body),
}

impl<A, P, S> Default for BaseRouterResponseBody<A, P, S> {
    fn default() -> Self {
        Self::Other(hyper::Body::empty())
    }
}

impl<A, P, S> http_body::Body for BaseRouterResponseBody<A, P, S>
where
    A: http_body::Body<Data = Bytes, Error = hyper::Error>,
    P: http_body::Body<Data = Bytes, Error = io::Error>,
    S: http_body::Body<Data = Bytes, Error = io::Error>,
{
    type Data = Bytes;
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project() {
            BaseRouterResponseBodyProj::Api(b) => b.poll_data(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::App(b) => b.poll_data(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::Assets(b) => b.poll_data(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::Other(b) => b.poll_data(cx).map_err(Into::into),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.project() {
            BaseRouterResponseBodyProj::Api(b) => b.poll_trailers(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::App(b) => b.poll_trailers(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::Assets(b) => b.poll_trailers(cx).map_err(Into::into),
            BaseRouterResponseBodyProj::Other(b) => b.poll_trailers(cx).map_err(Into::into),
        }
    }
}

#[derive(Clone)]
pub struct BaseRouter<A, P, S> {
    api: A,
    app: P,
    assets: S,
}

impl<A, P, S> BaseRouter<A, P, S> {
    pub fn new(api: A, app: P, assets: S) -> Self {
        Self { api, app, assets }
    }
}

impl<A, P, S, ReqBody, AResBody, PResBody, SResBody> Service<Request<ReqBody>>
    for BaseRouter<A, P, S>
where
    A: Service<Request<ReqBody>, Response = Response<AResBody>, Error = io::Error>,
    P: Service<Request<ReqBody>, Response = Response<PResBody>, Error = io::Error>,
    S: Service<Request<ReqBody>, Response = Response<SResBody>, Error = io::Error>,
    A::Future: Send + 'static,
    P::Future: Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<BaseRouterResponseBody<AResBody, PResBody, SResBody>>;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if let Poll::Ready(poll) = self.api.poll_ready(cx) {
            if let Err(e) = poll {
                return Poll::Ready(Err(e));
            }
            if let Poll::Ready(poll) = self.app.poll_ready(cx) {
                if let Err(e) = poll {
                    return Poll::Ready(Err(e));
                }
                if let Poll::Ready(poll) = self.assets.poll_ready(cx) {
                    if let Err(e) = poll {
                        return Poll::Ready(Err(e));
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
        Poll::Pending
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let uri = req.uri_mut();
        if let Some(path_and_query) = uri.path_and_query().map(PathAndQuery::as_str) {
            if let Some(rest) = path_and_query.strip_prefix("/api") {
                let mut parts = uri::Parts::default();
                parts.scheme = uri.scheme().cloned();
                parts.authority = uri.authority().cloned();
                parts.path_and_query =
                    PathAndQuery::from_maybe_shared(Bytes::copy_from_slice(rest.as_bytes())).ok();
                *uri = Uri::from_parts(parts).unwrap();
                return self
                    .api
                    .call(req)
                    .map(|r: Result<A::Response, A::Error>| {
                        r.map(|r| r.map(BaseRouterResponseBody::Api))
                    })
                    .boxed();
            }
            if let Some(rest) = path_and_query.strip_prefix("/lu-res") {
                let mut parts = uri::Parts::default();
                parts.scheme = uri.scheme().cloned();
                parts.authority = uri.authority().cloned();
                parts.path_and_query =
                    PathAndQuery::from_maybe_shared(Bytes::copy_from_slice(rest.as_bytes())).ok();
                *uri = Uri::from_parts(parts).unwrap();
                return self
                    .assets
                    .call(req)
                    .map(|r: Result<S::Response, S::Error>| {
                        r.map(|r| r.map(BaseRouterResponseBody::Assets))
                    })
                    .boxed();
            }
        }
        self.app
            .call(req)
            .map(|r: Result<P::Response, P::Error>| r.map(|r| r.map(BaseRouterResponseBody::App)))
            .boxed()
    }
}
