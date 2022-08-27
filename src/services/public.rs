use std::{
    io,
    path::Path,
    task::{Context, Poll},
};

use futures_util::{future::BoxFuture, FutureExt};
use http::{Request, Response, StatusCode};
use http_body::combinators::UnsyncBoxBody;
use hyper::body::{Bytes, HttpBody};
use tower::{Layer, Service};
use tower_http::services::{fs::DefaultServeDirFallback, ServeDir};

fn new_io_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

fn response_to_boxed_error_io<B: http_body::Body<Data = Bytes> + Send + 'static>(
    r: Response<B>,
) -> Response<ResponseBody>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    r.map(|b| HttpBody::map_err(b, new_io_error).boxed_unsync())
}

type ResponseBody = UnsyncBoxBody<Bytes, io::Error>;

pub struct PublicOrLayer<P> {
    path: P,
}

impl<P: AsRef<Path>> PublicOrLayer<P> {
    pub fn new(path: P) -> Self {
        Self { path }
    }
}

impl<S, P: AsRef<Path>> Layer<S> for PublicOrLayer<P> {
    type Service = PublicOr<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PublicOr::new(inner, &self.path)
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
    type Response = Response<ResponseBody>;
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
                            .map(response_to_boxed_error_io)
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
