use openapiv3::{OpenAPI, SecurityRequirement, Server};
use std::{convert::Infallible, future::Future, sync::Arc, task::Poll};
use warp::{
    filters::BoxedFilter,
    hyper::StatusCode,
    reply::{json, with_status, Json, WithStatus},
    Filter,
};

use crate::auth::AuthKind;

pub struct OpenApiFuture {
    /// The openapi structure
    inner: Arc<OpenAPI>,
}

impl Future for OpenApiFuture {
    type Output = Result<WithStatus<Json>, Infallible>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Poll::Ready(Ok(with_status(json(self.inner.as_ref()), StatusCode::OK)))
    }
}

/// Build the openapi endpoint
pub fn openapi(
    url: String,
    auth_kind: AuthKind,
) -> Result<BoxedFilter<(WithStatus<Json>,)>, serde_yaml::Error> {
    let text = include_str!("../../res/api.yaml");
    let mut data: OpenAPI = serde_yaml::from_str(text)?;
    data.servers.push(Server {
        url,
        description: Some(String::from("The current server")),
        ..Default::default()
    });
    if auth_kind == AuthKind::Basic {
        let mut req = SecurityRequirement::new();
        req.insert("basic_auth".to_string(), vec![]);
        data.security = Some(vec![req]);
    }
    let arc = Arc::new(data);
    Ok(warp::path("openapi.json")
        .and_then(move || OpenApiFuture { inner: arc.clone() })
        .boxed())
}
