use http::{header::AUTHORIZATION, Method};
use once_cell::sync::Lazy;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::CorsOptions;

pub trait CorsLayerExt<C> {
    fn configure(config: &C) -> Self;
}

static METHOD_QUERY: Lazy<Method> = Lazy::new(|| Method::from_bytes(b"QUERY").unwrap());

impl CorsLayerExt<CorsOptions> for CorsLayer {
    fn configure(cfg: &CorsOptions) -> Self {
        Self::new()
            .allow_headers([AUTHORIZATION])
            .allow_methods([
                Method::OPTIONS,
                Method::GET,
                Method::POST,
                METHOD_QUERY.clone(),
            ])
            .allow_origin(match cfg.all {
                true => AllowOrigin::any(),
                false => AllowOrigin::list(cfg.domains.clone()),
            })
    }
}
