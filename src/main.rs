use std::{
    fs::File,
    path::Path,
    str::FromStr,
    sync::{Arc, RwLock},
};

use api::ApiFactory;
use assembly_fdb::mem::Database;
use assembly_xml::localization::load_locale;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use config::{Config, Options};
use mapr::Mmap;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use paradox_typed_db::TypedDatabase;
use template::make_spa_dynamic;
use tokio::runtime::Handle;
use tracing::log::LevelFilter;
use warp::{
    filters::{header, BoxedFilter},
    hyper::Uri,
    path::FullPath,
    query, Filter, Reply,
};

mod api;
mod auth;
mod config;
mod data;
mod fallback;
mod redirect;
mod template;

use crate::{
    api::rev::ReverseLookup,
    auth::{AuthInfo, AuthKind, AuthQuery},
    config::AuthConfig,
    data::{fs::LuRes, locale::LocaleRoot},
    fallback::make_fallback,
    redirect::{add_host_filters, add_redirect_filters, base_filter},
    template::{load_meta_template, FsEventHandler, TemplateUpdateTask},
};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    pretty_env_logger::formatted_builder()
        .filter_level(LevelFilter::Info)
        .init();

    color_eyre::install()?;
    let opts = Options::parse();

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
    let canonical_base_url = if let Some(b) = cfg_g.base.as_deref() {
        format!("{}://{}/{}", scheme, &cfg_g.domain, b)
    } else {
        format!("{}://{}", scheme, &cfg_g.domain)
    };

    let lu_res = cfg
        .data
        .lu_res_prefix
        .clone()
        .unwrap_or_else(|| format!("{}/lu-res", canonical_base_url));

    let res = LuRes::new(&lu_res);

    let tydb = TypedDatabase::new(tables)?;
    let tydb = Box::leak(Box::new(tydb));
    let rev = Box::leak(Box::new(ReverseLookup::new(tydb)));

    // Load the files
    // let mut f = Folder::default();
    // if let Some(res) = &cfg.data.res {
    //     let mut loader = Loader::new();
    //     f = loader.load_dir(res);
    // }

    // Make the API
    let lu_json_path = cfg.data.lu_json_cache.clone();
    let fallback_routes = make_fallback(lu_json_path);

    let res_path = cfg
        .data
        .res
        .as_deref()
        .unwrap_or_else(|| Path::new("client/res"));
    let pki_path = cfg.data.versions.as_ref().map(|x| x.join("primary.pki"));

    let file_routes = warp::path("v1")
        .and(warp::path("res"))
        .and(data::fs::make_file_filter(res_path));

    let auth_kind = if matches!(cfg.auth, Some(AuthConfig { basic: Some(_), .. })) {
        AuthKind::Basic
    } else {
        AuthKind::None
    };
    let api_url = format!("{}/api/", canonical_base_url);
    let api_uri = Uri::from_str(&api_url).unwrap();

    let api_file = include_str!("../res/api.html");
    let api_swagger = warp::path::end()
        .and(warp::path::full())
        .map(move |path: FullPath| {
            if path.as_str().ends_with('/') {
                warp::reply::html(api_file).into_response()
            } else {
                warp::redirect(api_uri.clone()).into_response()
            }
        })
        .boxed();

    let api_routes = ApiFactory {
        url: api_url,
        auth_kind,
        db,
        tydb,
        rev,
        lr: lr.clone(),
        res_path,
        pki_path: pki_path.as_deref(),
    }
    .make_api();
    let api = warp::path("api").and(
        fallback_routes
            .or(api_swagger)
            .or(file_routes)
            .or(api_routes)
            .with(warp::compression::gzip()),
    );

    let spa_path = &cfg.data.explorer_spa;
    let spa_index = spa_path.join("index.html");

    // Create handlebars registry
    let hb = Arc::new(RwLock::new(template::Template::new()));

    load_meta_template(&hb, &spa_index)?;

    // Setup the watcher
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let eh = FsEventHandler::new(tx);
    let mut watcher = recommended_watcher(eh)?;
    watcher.watch(&spa_index, RecursiveMode::Recursive).unwrap();

    let rt = Handle::current();

    rt.spawn(TemplateUpdateTask::new(rx, hb.clone()));

    let loc = LocaleRoot::new(lr);
    let spa_dynamic = make_spa_dynamic(tydb, loc, res, hb, &cfg.general.domain);

    //let spa_file = warp::fs::file(spa_index);
    let spa = warp::fs::dir(spa_path.clone()).or(spa_dynamic);

    // Initialize the lu-res cache
    let res = warp::path("lu-res")
        .and(warp::fs::dir(cfg.data.lu_res_cache.clone()))
        .boxed();

    // Finally collect all routes
    let routes = res.or(api).or(spa);

    let auth_fn = auth::make_auth_fn(&cfg.auth);

    let auth = header::optional::<String>("Authorization")
        .and(header::optional("User-Agent"))
        .and(query::query::<AuthQuery>())
        .map(AuthInfo::new)
        .and_then(auth_fn);

    // Add a public route
    let public = warp::fs::dir(cfg.data.public.clone());

    let routes = public.or(auth).or(routes);

    let mut root = base_filter(cfg.general.base.as_deref()).boxed();

    root = add_host_filters(root, &cfg.host);

    let routes = root.and(routes).boxed();

    let mut redirect: BoxedFilter<(_,)> = warp::any()
        .and_then(|| async move { Err(warp::reject()) })
        .boxed();

    redirect = add_redirect_filters(redirect, &cfg);

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
    cors = cors
        .allow_methods(vec!["OPTIONS", "GET"])
        .allow_headers(vec!["authorization"]);
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
