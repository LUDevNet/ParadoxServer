use std::{
    io,
    path::Path,
    str::FromStr,
    task::{Context, Poll},
};

use http::{
    uri::{self, PathAndQuery},
    Request as HttpRequest, Response as HttpResponse, Uri,
};
use hyper::body::Bytes;
use tower::Service;
use tower_http::services::{
    fs::{DefaultServeDirFallback, ServeFileSystemResponseBody, ServeFileSystemResponseFuture},
    ServeDir,
};

/// This service takes care of everything that the API pretends to
/// do itself right now, but actually doesn't
#[derive(Clone)]
pub struct FallbackService {
    inner: ServeDir,
}

impl FallbackService {
    pub fn new(lu_json_path: &Path) -> Self {
        Self {
            inner: ServeDir::new(lu_json_path),
        }
    }

    /// The routes that require the fallback
    pub(super) fn requires_fallback(path: &str) -> bool {
        path.starts_with("/api/v0/maps/") || path.starts_with("/api/v0/scripts/")
    }

    /// The prefix path to remove from the request before passing to [ServeDir]
    const PREFIX: &'static str = "/api/v0";
}

impl<B> Service<HttpRequest<B>> for FallbackService
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = HttpResponse<ServeFileSystemResponseBody>;
    type Error = io::Error;
    type Future = ServeFileSystemResponseFuture<B, DefaultServeDirFallback>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <ServeDir as Service<HttpRequest<B>>>::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, req: HttpRequest<B>) -> Self::Future {
        let path = req.uri().path();
        let mut parts = uri::Parts::default();
        parts.authority = req.uri().authority().cloned();
        parts.scheme = req.uri().scheme().cloned();

        let suffix = path
            .strip_prefix(Self::PREFIX)
            .expect("Should not reach this for other paths");
        parts.path_and_query = Some(PathAndQuery::from_str(suffix).unwrap());
        let (mut req_parts, body) = req.into_parts();
        req_parts.uri = Uri::from_parts(parts).unwrap();
        let req = HttpRequest::from_parts(req_parts, body);

        self.inner.call(req)
    }
}
