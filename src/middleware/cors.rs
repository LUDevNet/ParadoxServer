use http::{header::AUTHORIZATION, Method};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::CorsOptions;

pub trait CorsLayerExt<C> {
    fn configure(config: &C) -> Self;
}

impl CorsLayerExt<CorsOptions> for CorsLayer {
    fn configure(cfg: &CorsOptions) -> Self {
        Self::new()
            .allow_headers([AUTHORIZATION])
            .allow_methods([Method::OPTIONS, Method::GET])
            .allow_origin(match cfg.all {
                true => AllowOrigin::any(),
                false => AllowOrigin::list(cfg.domains.clone()),
            })
    }
}
