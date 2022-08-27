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
use http_body::Body as HttpBody;
use hyper::body::Bytes;
use pin_project::pin_project;
use tower::Service;
use tower_http::services::fs::ServeFileSystemResponseBody;

use super::{Error, FallbackService};

#[pin_project(project = BaseRouterResponseBodyProj)]
pub enum BaseRouterResponseBody<A, P, S> {
    Api(#[pin] A),
    App(#[pin] P),
    Assets(#[pin] S),
    Fallback(#[pin] ServeFileSystemResponseBody),
    Other(#[pin] hyper::Body),
}

impl<A, P, S> Default for BaseRouterResponseBody<A, P, S> {
    fn default() -> Self {
        Self::Other(hyper::Body::empty())
    }
}

impl<A, P, S> HttpBody for BaseRouterResponseBody<A, P, S>
where
    A: HttpBody<Data = Bytes, Error = hyper::Error>,
    P: HttpBody<Data = Bytes, Error = io::Error>,
    S: HttpBody<Data = Bytes, Error = io::Error>,
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
            BaseRouterResponseBodyProj::Fallback(b) => b.poll_data(cx).map_err(Into::into),
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
            BaseRouterResponseBodyProj::Fallback(b) => b.poll_trailers(cx).map_err(Into::into),
        }
    }
}

#[derive(Clone)]
pub struct BaseRouter<A, P, S> {
    api: A,
    app: P,
    res: S,
    fallback: FallbackService,
}

pub const RES_PREFIX: &str = "/lu-res";

impl<A, P, S> BaseRouter<A, P, S> {
    pub fn new(api: A, app: P, res: S, fallback: FallbackService) -> Self {
        Self {
            api,
            app,
            res,
            fallback,
        }
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
    ReqBody: HttpBody<Data = Bytes> + Send + 'static,
    ReqBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
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
                if let Poll::Ready(poll) = self.res.poll_ready(cx) {
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
            if FallbackService::requires_fallback(path_and_query) {
                return self
                    .fallback
                    .call(req)
                    .map(
                        |r: Result<http::Response<ServeFileSystemResponseBody>, io::Error>| {
                            r.map(|r| r.map(BaseRouterResponseBody::Fallback))
                        },
                    )
                    .boxed();
            }
            if let Some(rest) = path_and_query.strip_prefix("/api") {
                let mut parts = uri::Parts::default();
                parts.scheme = uri.scheme().cloned();
                parts.authority = uri.authority().cloned();
                let src_path_bytes = Bytes::copy_from_slice(rest.as_bytes());
                parts.path_and_query = PathAndQuery::from_maybe_shared(src_path_bytes).ok();
                *uri = Uri::from_parts(parts).unwrap();
                return self
                    .api
                    .call(req)
                    .map(|r: Result<A::Response, A::Error>| {
                        r.map(|r| r.map(BaseRouterResponseBody::Api))
                    })
                    .boxed();
            }
            if let Some(rest) = path_and_query.strip_prefix(RES_PREFIX) {
                let mut parts = uri::Parts::default();
                parts.scheme = uri.scheme().cloned();
                parts.authority = uri.authority().cloned();
                parts.path_and_query =
                    PathAndQuery::from_maybe_shared(Bytes::copy_from_slice(rest.as_bytes())).ok();
                *uri = Uri::from_parts(parts).unwrap();
                return self
                    .res
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
