use openapiv3::{OpenAPI, SecurityRequirement, Server};
use std::sync::Arc;

use crate::auth::AuthKind;

#[derive(Clone)]
pub struct OpenApiService {
    /// The openapi structure
    inner: Arc<OpenAPI>,
}

impl AsRef<OpenAPI> for OpenApiService {
    fn as_ref(&self) -> &OpenAPI {
        self.inner.as_ref()
    }
}

impl OpenApiService {
    pub fn new(url: &str, auth_kind: AuthKind) -> Result<Self, serde_yaml::Error> {
        let text = include_str!("../../res/api.yaml");
        let mut data: OpenAPI = serde_yaml::from_str(text)?;
        data.servers.push(Server {
            url: url.to_string(),
            description: Some(String::from("The current server")),
            ..Default::default()
        });
        if auth_kind == AuthKind::Basic {
            let mut req = SecurityRequirement::new();
            req.insert("basic_auth".to_string(), vec![]);
            data.security = Some(vec![req]);
        }
        Ok(Self {
            inner: Arc::new(data),
        })
    }
}
