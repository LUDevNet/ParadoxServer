use std::{
    borrow::Cow,
    fs::File,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use api::make_api;
use assembly_data::{fdb::mem::Database, xml::localization::load_locale};
use color_eyre::eyre::WrapErr;
use config::{AuthConfig, Config, Options};
use handlebars::Handlebars;
use mapr::Mmap;
use paradox_typed_db::TypedDatabase;
use regex::{Captures, Regex};
use structopt::StructOpt;
use template::make_spa_dynamic;
use tracing::info;
use warp::{
    filters::BoxedFilter,
    hyper::{StatusCode, Uri},
    path::Tail,
    reply::{with_header, with_status, Html, WithHeader, WithStatus},
    Filter, Future, Rejection,
};

mod api;
mod config;
mod data;
mod template;

use crate::api::rev_lookup::ReverseLookup;

fn make_meta_template(text: &str) -> Cow<str> {
    let re = Regex::new("<meta\\s+(name|property)=\"(.*?)\"\\s+content=\"(.*)\"\\s*/?>").unwrap();
    re.replace_all(text, |cap: &Captures| {
        let kind = &cap[1];
        let name = &cap[2];
        let value = match name {
            "twitter:title" | "og:title" => "{{title}}",
            "twitter:description" | "og:description" => "{{description}}",
            "twitter:image" | "og:image" => "{{image}}",
            "og:url" => "{{url}}",
            "og:type" => "{{type}}",
            "twitter:card" => "{{card}}",
            "twitter:site" => "{{site}}",
            _ => &cap[3],
        };
        format!("<meta {}=\"{}\" content=\"{}\">", kind, name, value)
    })
}

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

fn make_auth_fn(cfg: &Option<AuthConfig>) -> impl Fn(Option<String>) -> CheckFuture + Clone {
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

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    pretty_env_logger::init();

    color_eyre::install()?;
    let opts = Options::from_args();

    let cfg_path = opts.cfg;
    let cfg_file = std::fs::read_to_string(&cfg_path)
        .wrap_err_with(|| format!("Failed to open config file '{}'", cfg_path.display()))?;
    let cfg: Config = toml::from_str(&cfg_file)?;

    // Load the database file
    let file = File::open(&cfg.data.cdclient).wrap_err_with(|| {
        format!(
            "Failed to open input file '{}'",
            cfg.data.cdclient.display()
        )
    })?;

    // Load the database
    let mmap = unsafe { Mmap::map(&file)? };
    // We want to keep this mapped until the end of the program!
    let mref: &'static Mmap = Box::leak(Box::new(mmap));
    let buf: &'static [u8] = mref.as_ref();
    let db = Database::new(buf);

    // Load the locale
    let locale_root = load_locale(&cfg.data.locale).context("Failed to load locale.xml")?;
    let lr = Arc::new(locale_root);

    // Load the typed database
    let tables = db.tables().unwrap();

    let scheme = match cfg.general.secure {
        true => "https",
        false => "http",
    };

    let cfg_g = &cfg.general;
    let lu_res = cfg.data.lu_res_prefix.unwrap_or_else(|| {
        if let Some(b) = &cfg_g.base {
            format!("{}://{}/{}/lu-res", scheme, &cfg_g.domain, b)
        } else {
            format!("{}://{}/lu-res", scheme, &cfg_g.domain)
        }
    });

    let lu_res_prefix = Box::leak(lu_res.clone().into_boxed_str());
    let data = Box::leak(Box::new(TypedDatabase::new(
        lr.clone(),
        lu_res_prefix,
        tables,
    )));
    let rev = Box::leak(Box::new(ReverseLookup::new(data)));

    // Make the API
    let api = warp::path("api").and(make_api(db, data, rev, lr.clone()));

    let spa_path = cfg.data.explorer_spa;
    let spa_index = spa_path.join("index.html");

    let index_text = std::fs::read_to_string(&spa_index)?;
    let index_tpl_str = make_meta_template(&index_text);

    let mut hb = Handlebars::new();
    // register the template
    hb.register_template_string("template.html", index_tpl_str)?;

    // Turn Handlebars instance into a Filter so we can combine it
    // easily with others...
    let hb = Arc::new(hb);

    let spa_dynamic = make_spa_dynamic(data, hb, &cfg.general.domain);

    //let spa_file = warp::fs::file(spa_index);
    let spa = warp::fs::dir(spa_path).or(spa_dynamic);

    // Initialize the lu-res cache
    let res = warp::path("lu-res")
        .and(warp::fs::dir(cfg.data.lu_res_cache))
        .boxed();

    // Finally collect all routes
    let routes = res.or(api).or(spa);

    let auth_fn = make_auth_fn(&cfg.auth);

    let auth = warp::filters::header::optional::<String>("Authorization").and_then(auth_fn);

    let routes = auth.or(routes);

    fn base_filter(
        b: Option<String>,
    ) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
        if let Some(b) = b {
            let base = warp::path(b);
            warp::get().and(base).boxed()
        } else {
            warp::get().boxed()
        }
    }

    let canonical_domain = cfg.general.domain.clone();
    let canonical_base = cfg.general.base.clone().unwrap_or_default();

    let mut root = base_filter(cfg.general.base).boxed();

    for host in &cfg.host {
        let base = base_filter(host.base.clone());
        if !host.redirect {
            info!("Loading host {:?}", host);
            root = warp::filters::host::exact(&host.name)
                .and(base)
                .or(root)
                .unify()
                .boxed();
        }
    }

    let routes = root.and(routes).boxed();

    let mut redirect: BoxedFilter<(_,)> = warp::any()
        .and_then(|| async move { Err(warp::reject()) })
        .boxed();

    for host in cfg.host {
        if host.redirect {
            info!("Loading redirect {:?}", host);
            let base = base_filter(host.base);
            let dom = canonical_domain.clone();
            let bas = canonical_base.clone();
            let new_redirect = base
                .and(warp::filters::path::tail())
                .map(move |path: Tail| {
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
                    warp::redirect::permanent(uri)
                })
                .boxed();
            redirect = redirect.or(new_redirect).unify().boxed();
        }
    }

    let routes = redirect.or(routes);
    let log = warp::log("paradox::routes");
    let routes = routes.with(log);

    let ip = if cfg.general.public {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };

    let mut cors = warp::cors();
    let cors_cfg = &cfg.general.cors;
    if cors_cfg.all {
        cors = cors.allow_any_origin();
    } else {
        for key in &cors_cfg.domains {
            cors = cors.allow_origin(key.as_ref());
        }
    }
    cors = cors.allow_methods(vec!["GET"]);
    let to_serve = routes.with(cors);
    let server = warp::serve(to_serve);

    if let Some(tls_cfg) = cfg.tls {
        if tls_cfg.enabled {
            server
                .tls()
                .key_path(tls_cfg.key)
                .cert_path(tls_cfg.cert)
                .run((ip, cfg.general.port))
                .await;
            return Ok(());
        }
    }
    server.run((ip, cfg.general.port)).await;

    Ok(())
}
