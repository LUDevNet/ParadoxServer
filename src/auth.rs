use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use warp::{
    hyper::StatusCode,
    reply::{with_header, with_status, Html, WithHeader, WithStatus},
    Future, Rejection,
};

use crate::config::AuthConfig;

#[derive(Clone)]
pub enum AuthImpl {
    None,
    Basic(Arc<Vec<String>>),
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
    fn check(&self, auth: Option<String>) -> CheckFuture {
        match self {
            Self::None => return CheckFuture { is_allowed: true },
            Self::Basic(allowed) => {
                if let Some(header) = auth {
                    if let Some(hash) = header.strip_prefix("Basic ") {
                        if allowed.iter().any(|x| x == hash) {
                            return CheckFuture { is_allowed: true };
                        }
                    }
                }
            }
        }
        CheckFuture { is_allowed: false }
    }
}

pub fn make_auth_fn(cfg: &Option<AuthConfig>) -> impl Fn(Option<String>) -> CheckFuture + Clone {
    let mut auth_impl = AuthImpl::None;
    if let Some(auth_cfg) = cfg {
        if let Some(basic_auth_cfg) = &auth_cfg.basic {
            let accounts: Vec<String> = basic_auth_cfg
                .iter()
                .map(|(user, password)| {
                    let text = format!("{}:{}", user, password);
                    base64::encode(&text)
                })
                .collect();
            auth_impl = AuthImpl::Basic(Arc::new(accounts));
        }
    }
    move |v: Option<String>| auth_impl.check(v)
}
