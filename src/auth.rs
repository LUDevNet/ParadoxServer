use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use serde::Deserialize;
use warp::{
    hyper::StatusCode,
    reply::{with_header, with_status, Html, WithHeader, WithStatus},
    Future, Rejection,
};

use crate::config::AuthConfig;

#[derive(Deserialize)]
pub struct AuthQuery {
    #[serde(default, rename = "apiKey")]
    api_key: Option<String>,
}

pub struct AuthInfo {
    authorization: Option<String>,
    user_agent: Option<String>,
    query: AuthQuery,
}

impl AuthInfo {
    pub fn new(
        authorization: Option<String>,
        user_agent: Option<String>,
        query: AuthQuery,
    ) -> Self {
        Self {
            authorization,
            user_agent,
            query,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// No authentication
    None,
    /// HTTP Basic Auth
    Basic,
}

pub struct BasicCfg {
    allowed_credentials: Vec<String>,
    allowed_bots: Vec<String>,
    allowed_api_keys: Vec<String>,
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
    fn check(&self, auth: AuthInfo) -> CheckFuture {
        match self {
            Self::None => return CheckFuture { is_allowed: true },
            Self::Basic(cfg) => {
                if let Some(header) = auth.authorization {
                    if let Some(hash) = header.strip_prefix("Basic ") {
                        if cfg.allowed_credentials.iter().any(|x| x == hash) {
                            return CheckFuture { is_allowed: true };
                        }
                    }
                }
                if let Some(api_key) = &auth.query.api_key {
                    if cfg.allowed_api_keys.contains(api_key) {
                        return CheckFuture { is_allowed: true };
                    }
                }
                if let Some(ua) = auth.user_agent {
                    for allowed in cfg.allowed_bots.iter() {
                        if ua.contains(allowed) {
                            return CheckFuture { is_allowed: true };
                        }
                    }
                }
            }
        }
        CheckFuture { is_allowed: false }
    }
}

pub fn make_auth_fn(cfg: &Option<AuthConfig>) -> impl Fn(AuthInfo) -> CheckFuture + Clone {
    let mut auth_impl = AuthImpl::None;
    if let Some(auth_cfg) = cfg {
        if let Some(basic_auth_cfg) = &auth_cfg.basic {
            let allowed_credentials: Vec<String> = basic_auth_cfg
                .iter()
                .map(|(user, password)| {
                    let text = format!("{}:{}", user, password);
                    base64::encode(&text)
                })
                .collect();
            auth_impl = AuthImpl::Basic(Arc::new(BasicCfg {
                allowed_credentials,
                allowed_bots: auth_cfg.user_agents.clone(),
                allowed_api_keys: auth_cfg.api_keys.clone(),
            }));
        }
    }
    move |v: AuthInfo| auth_impl.check(v)
}
