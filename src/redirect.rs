use tracing::info;
use warp::{
    filters::BoxedFilter,
    http::HeaderValue,
    hyper::{body::Bytes, header, StatusCode, Uri},
    path::Tail,
    reply::{self, WithHeader},
    Filter,
};

use crate::config::{Config, HostConfig};

pub fn base_filter(
    b: Option<&str>,
) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone + 'static {
    if let Some(b) = b {
        let base = warp::path(b.to_owned());
        warp::get().and(base).boxed()
    } else {
        warp::get().boxed()
    }
}

pub fn redirect_route(
    dom: String,
    bas: String,
) -> impl (Fn(Tail) -> WithHeader<StatusCode>) + Clone {
    move |path: Tail| {
        let mut new_path = String::from("/");
        new_path.push_str(&bas);
        if !new_path.ends_with('/') {
            new_path.push('/');
        }
        new_path.push_str(path.as_str());
        let uri = Uri::builder()
            .scheme("https")
            .authority(dom.as_str())
            .path_and_query(&new_path)
            .build()
            .unwrap();

        let bytes = Bytes::from(uri.to_string());
        reply::with_header(
            StatusCode::PERMANENT_REDIRECT,
            header::LOCATION,
            HeaderValue::from_maybe_shared(bytes).unwrap(),
        )
    }
}

pub fn add_host_filters(root: BoxedFilter<()>, host_cfg: &[HostConfig]) -> BoxedFilter<()> {
    let mut root = root;
    for host in host_cfg {
        let base = base_filter(host.base.as_deref());
        if !host.redirect {
            info!("Loading host {:?}", host);
            root = warp::filters::host::exact(&host.name)
                .and(base)
                .or(root)
                .unify()
                .boxed();
        }
    }
    root
}

type RedirectFilter = BoxedFilter<(WithHeader<StatusCode>,)>;
pub fn add_redirect_filters(mut redirect: RedirectFilter, cfg: &Config) -> RedirectFilter {
    let canonical_domain = cfg.general.domain.clone();
    let canonical_base = cfg.general.base.clone().unwrap_or_default();

    for host in &cfg.host {
        if host.redirect {
            info!("Loading redirect {:?}", host);
            let base = base_filter(host.base.as_deref());
            let new_redirect = base
                .and(warp::filters::path::tail())
                .map(redirect_route(
                    canonical_domain.clone(),
                    canonical_base.clone(),
                ))
                .boxed();
            redirect = redirect.or(new_redirect).unify().boxed();
        }
    }
    redirect
}
