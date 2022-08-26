use std::{
    io,
    path::Path,
    str::FromStr,
    task::{Context, Poll},
};

use http::uri::{self, PathAndQuery};
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
    maps: ServeDir,
    scripts: ServeDir,
}

impl FallbackService {
    pub fn new(lu_json_path: &Path) -> Self {
        Self {
            maps: ServeDir::new(lu_json_path.join("maps")),
            scripts: ServeDir::new(lu_json_path.join("scripts")),
        }
    }

    pub(super) fn requires_fallback(path: &str) -> bool {
        path.starts_with("/api/v0/maps/") || path.starts_with("/api/v0/scripts/")
    }
}

impl<B> Service<http::Request<B>> for FallbackService
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = http::Response<ServeFileSystemResponseBody>;
    type Error = io::Error;
    type Future = ServeFileSystemResponseFuture<B, DefaultServeDirFallback>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // [ServeDir] is always ready
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let path = req.uri().path();
        let mut parts = uri::Parts::default();
        parts.authority = req.uri().authority().cloned();
        parts.scheme = req.uri().scheme().cloned();

        let (handler, path) = if let Some(suffix) = path.strip_prefix("/api/v0/maps") {
            (&mut self.maps, suffix)
        } else if let Some(suffix) = path.strip_prefix("/api/v0/scripts") {
            (&mut self.scripts, suffix)
        } else {
            panic!("Should not reach this for other paths")
        };
        parts.path_and_query = Some(PathAndQuery::from_str(path).unwrap());
        let (mut req_parts, body) = req.into_parts();
        req_parts.uri = http::Uri::from_parts(parts).unwrap();
        let req = http::Request::from_parts(req_parts, body);

        handler.call(req)
    }
}
