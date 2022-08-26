use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{future::BoxFuture, FutureExt};
use http::{
    uri::{self, PathAndQuery},
    Request, Response, Uri,
};
use hyper::body::Bytes;
use pin_project::pin_project;
use tower::Service;

use super::Error;

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
