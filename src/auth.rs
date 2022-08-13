use std::{
    collections::HashSet,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use http::{
    header::{AUTHORIZATION, USER_AGENT, WWW_AUTHENTICATE},
    HeaderValue, Request, Response,
};
use tower_http::auth::AuthorizeRequest;
use warp::{
    hyper::StatusCode,
    reply::{with_header, with_status, Html, WithHeader, WithStatus},
    Future, Rejection,
};

use crate::config::AuthConfig;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// No authentication
    None,
    /// HTTP Basic Auth
    Basic,
}

pub struct BasicCfg {
    allowed_credentials: HashSet<String>,
    allowed_bots: HashSet<String>,
    allowed_api_keys: HashSet<String>,
}

#[derive(Clone)]
pub enum AuthImpl {
    None,
    Basic(Arc<BasicCfg>),
}

pub struct CheckFuture {
    is_allowed: bool,
}

type CheckResult = Result<WithStatus<WithHeader<Html<&'static str>>>, Rejection>;

impl CheckFuture {
    fn get(&self) -> CheckResult {
        if self.is_allowed {
            Err(warp::reject()) // and fall through to the app
        } else {
            Ok(with_status(
                with_header(
                    warp::reply::html("Access denied"),
                    "WWW-Authenticate",
                    "Basic realm=\"LU-Explorer\"",
                ),
                StatusCode::UNAUTHORIZED,
            ))
        }
    }
}

impl Future for CheckFuture {
    type Output = CheckResult;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(self.get())
    }
}

impl AuthImpl {
    pub fn new(cfg: Option<&AuthConfig>) -> Self {
        let mut auth_impl = AuthImpl::None;
        if let Some(auth_cfg) = cfg {
            if let Some(basic_auth_cfg) = &auth_cfg.basic {
                let allowed_credentials: HashSet<String> = basic_auth_cfg
                    .iter()
                    .map(|(user, password)| {
                        let text = format!("{}:{}", user, password);
                        base64::encode(&text)
                    })
                    .collect();
                auth_impl = AuthImpl::Basic(Arc::new(BasicCfg {
                    allowed_credentials,
                    allowed_bots: auth_cfg.user_agents.iter().cloned().collect(),
                    allowed_api_keys: auth_cfg.api_keys.iter().cloned().collect(),
                }));
            }
        }
        auth_impl
    }
}

impl<R> Clone for Authorize<R> {
    fn clone(&self) -> Self {
        Self {
            _p: self._p,
            kind: self.kind.clone(),
        }
    }
}

pub struct Authorize<R> {
    kind: AuthImpl,
    _p: PhantomData<fn() -> R>,
}

impl<R> Authorize<R> {
    pub fn new(cfg: &Option<AuthConfig>) -> Self {
        Self {
            kind: AuthImpl::new(cfg.as_ref()),
            _p: PhantomData,
        }
    }
}

impl<B: http_body::Body, R: http_body::Body + Default> AuthorizeRequest<B> for Authorize<R> {
    type ResponseBody = R;

    fn authorize(&mut self, request: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
        match &self.kind {
            AuthImpl::None => Ok(()),
            AuthImpl::Basic(cfg) => {
                if let Some(Ok(authorization)) = request
                    .headers()
                    .get(AUTHORIZATION)
                    .map(HeaderValue::to_str)
                {
                    if let Some(credentials) = authorization.strip_prefix("Basic ") {
                        if cfg.allowed_credentials.contains(credentials) {
                            return Ok(());
                        }
                    }
                }
                if let Some(query) = request.uri().query() {
                    let parse = form_urlencoded::parse(query.as_bytes());
                    for (key, value) in parse {
                        if key == "apiKey" && cfg.allowed_api_keys.contains(value.as_ref()) {
                            return Ok(());
                        }
                    }
                }
                if let Some(Ok(user_agent)) =
                    request.headers().get(USER_AGENT).map(HeaderValue::to_str)
                {
                    if cfg.allowed_bots.contains(user_agent) {
                        return Ok(());
                    }
                }
                let mut response = Response::new(R::default());
                *response.status_mut() = StatusCode::UNAUTHORIZED;
                response.headers_mut().append(
                    WWW_AUTHENTICATE,
                    HeaderValue::from_static("Basic realm=\"LU-Explorer\""),
                );
                Err(response)
            }
        }
    }
}
