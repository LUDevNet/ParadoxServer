use std::{
    collections::HashMap,
    future::{Future, Ready},
    io,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
};

use http::HeaderValue;
use http::{header::LOCATION, uri::PathAndQuery};
use hyper::{body::Bytes, StatusCode, Uri};
use pin_project::pin_project;
use tower::{Layer, Service};

use crate::config::Config;

/// `dom` and `base` are the canonical / target values
fn redirect_location(domain: &str, base: &str, path: &str) -> HeaderValue {
    let mut new_path = String::from("/");
    new_path.push_str(base);
    if !new_path.ends_with('/') {
        new_path.push('/');
    }
    new_path.push_str(path);
    let uri = Uri::builder()
        .scheme("https")
        .authority(domain)
        .path_and_query(&new_path)
        .build()
        .unwrap();

    let bytes = Bytes::from(uri.to_string());
    HeaderValue::from_maybe_shared(bytes).unwrap()
}

struct RedirectPolicy {
    redirect: bool,
    base: Option<String>,
}

struct RedirectCore {
    hosts: HashMap<String, RedirectPolicy>,
    canonical_domain: String,
    canonical_base: String,
}

impl RedirectCore {
    pub fn new(cfg: &Config) -> Self {
        let canonical_domain = cfg.general.domain.clone();
        let canonical_base = cfg.general.base.clone().unwrap_or_default();
        let mut hosts = HashMap::with_capacity(cfg.host.len());
        for cfg in &cfg.host {
            hosts.insert(
                cfg.name.clone(),
                RedirectPolicy {
                    redirect: cfg.redirect,
                    base: cfg.base.clone(),
                },
            );
        }
        Self {
            hosts,
            canonical_domain,
            canonical_base,
        }
    }
}

pub struct RedirectLayer {
    core: Arc<RedirectCore>,
}

impl RedirectLayer {
    pub fn new(cfg: &Config) -> Self {
        Self {
            core: Arc::new(RedirectCore::new(cfg)),
        }
    }
}

impl<S> Layer<S> for RedirectLayer {
    type Service = Redirect<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Redirect {
            inner,
            core: self.core.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Redirect<S> {
    inner: S,
    core: Arc<RedirectCore>,
}

#[pin_project(project = RedirectFutureProj)]
pub enum RedirectFuture<F, B> {
    Inner(#[pin] F),
    Ready(#[pin] Ready<http::Response<B>>),
}

impl<F, B> Future for RedirectFuture<F, B>
where
    F: Future<Output = Result<http::Response<B>, io::Error>>,
{
    type Output = Result<http::Response<B>, io::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            RedirectFutureProj::Inner(f) => f.poll(cx),
            RedirectFutureProj::Ready(f) => f.poll(cx).map(Ok),
        }
    }
}

impl<B, S, ResBody> Service<http::Request<B>> for Redirect<S>
where
    S: Service<http::Request<B>, Response = http::Response<ResBody>, Error = io::Error>,
    ResBody: Default,
{
    type Response = http::Response<ResBody>;
    type Error = io::Error;
    type Future = RedirectFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        if let Some(host) = req.uri().authority() {
            let domain = host.as_str();
            if let Some(policy) = self.core.hosts.get(domain) {
                let mut path = req.uri().path();
                if let Some(base) = policy.base.as_deref() {
                    if let Some(inner_path) = path.strip_prefix(base) {
                        path = inner_path;
                    } else {
                        let mut r = http::Response::new(ResBody::default());
                        *r.status_mut() = StatusCode::NOT_FOUND;
                        return RedirectFuture::Ready(std::future::ready(r));
                    }
                }

                if policy.redirect {
                    let mut r = http::Response::new(ResBody::default());
                    let location = redirect_location(
                        &self.core.canonical_domain,
                        &self.core.canonical_base,
                        path,
                    );
                    *r.status_mut() = StatusCode::PERMANENT_REDIRECT;
                    r.headers_mut().append(LOCATION, location);
                    return RedirectFuture::Ready(std::future::ready(r));
                } else {
                    // Note: get the byte first, so we can stop borrowing `path`.
                    let path_bytes = Bytes::from(path.to_string());
                    let pnq = PathAndQuery::from_maybe_shared(path_bytes).unwrap();

                    let (mut parts, body) = req.into_parts();
                    let mut uri_parts = parts.uri.into_parts();

                    uri_parts.path_and_query = Some(pnq);

                    parts.uri = Uri::from_parts(uri_parts).unwrap();
                    req = http::Request::from_parts(parts, body);
                }
            }
        }
        RedirectFuture::Inner(self.inner.call(req))
    }
}
