use crate::{
    api::{rev::ReverseLookup, ApiService},
    auth::{AuthKind, Authorize},
    config::{Config, Options},
    data::{fs::LuRes, locale::LocaleRoot},
    redirect::RedirectLayer,
    services::{BaseRouter, FallbackService, PublicOrLayer},
    template::{load_meta_template, spawn_watcher},
};
use assembly_fdb::mem::Database;
use assembly_xml::localization::load_locale;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use http::{header::AUTHORIZATION, Method, Uri};
use hyper::server::Server;
use mapr::Mmap;
use paradox_typed_db::TypedDatabase;
use std::{
    fs::File,
    net::SocketAddr,
    path::Path,
    str::FromStr,
    sync::{Arc, RwLock},
};
use tower::{make::Shared, ServiceBuilder};
use tower_http::{
    auth::RequireAuthorizationLayer,
    cors::{AllowOrigin, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::log::LevelFilter;

mod api;
mod auth;
mod config;
mod data;
mod redirect;
mod services;
mod template;

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
    let canonical_base_url = match cfg_g.base.as_deref() {
        Some(b) => format!("{}://{}/{}", scheme, &cfg_g.domain, b),
        None => format!("{}://{}", scheme, &cfg_g.domain),
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

    // Make the API
    let lu_json_path = cfg.data.lu_json_cache.as_path();

    // The 'new' fallback service
    let fallback = FallbackService::new(lu_json_path);

    let res_path = cfg
        .data
        .res
        .as_deref()
        .unwrap_or_else(|| Path::new("client/res"));

    let pki_path = cfg.data.versions.as_ref().map(|x| x.join("primary.pki"));

    let auth_kind = AuthKind::of(&cfg.auth);
    let api_url = format!("{}/api/", canonical_base_url);
    let api_uri = Uri::from_str(&api_url).unwrap();

    let openapi = api::docs::OpenApiService::new(&api_url, auth_kind)?;
    let pack = api::files::PackService::new(res_path, pki_path.as_deref())?;
    let api = ApiService::new(db, lr.clone(), pack, openapi, api_uri, tydb, rev, res_path);

    let spa_path = &cfg.data.explorer_spa;
    let spa_index = spa_path.join("index.html");

    // Create handlebars registry
    let hb = Arc::new(RwLock::new(template::Template::new()));
    load_meta_template(&hb, &spa_index)?;
    spawn_watcher(&spa_index, hb.clone())?;

    // Set up the application
    let spa_dynamic =
        template::SpaDynamic::new(tydb, LocaleRoot::new(lr), res, hb, &cfg.general.domain);
    let app = ServeDir::new(spa_path)
        .append_index_html_on_directories(false)
        .fallback(spa_dynamic);

    // Initialize the lu-res cache
    let res = ServeDir::new(&cfg.data.lu_res_cache);

    let cors_cfg = &cfg.general.cors;
    let allow_origin = if cors_cfg.all {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(cors_cfg.domains.clone())
    };

    let service = ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_headers([AUTHORIZATION])
                .allow_methods([Method::OPTIONS, Method::GET])
                .allow_origin(allow_origin),
        )
        .layer(RedirectLayer::new(&cfg))
        .layer(PublicOrLayer::new(&cfg.data.public))
        .layer(RequireAuthorizationLayer::custom(Authorize::new(&cfg.auth)))
        .service(BaseRouter::new(api, app, res, fallback));

    let ip = match cfg.general.public {
        true => [0, 0, 0, 0],
        false => [127, 0, 0, 1],
    };

    // FIXME: TLS
    /*if let Some(tls_cfg) = cfg.tls {
        if tls_cfg.enabled {
            server
                .tls()
                .key_path(tls_cfg.key)
                .cert_path(tls_cfg.cert)
                .run((ip, cfg.general.port))
                .await;
            return Ok(());
        }
    }*/

    // And run our service using `hyper`
    let addr = SocketAddr::from((ip, cfg.general.port));
    Server::bind(&addr)
        .serve(Shared::new(service))
        .await
        .expect("server error");

    Ok(())
}
