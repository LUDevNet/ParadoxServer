use crate::{
    api::rev::ReverseLookup,
    auth::{AuthKind, Authorize},
    config::{Config, Options},
    data::{fs::LuRes, locale::LocaleRoot},
    middleware::{PublicOrLayer, RedirectLayer},
    services::{router, BaseRouter, FallbackService},
};
use assembly_fdb::mem::Database;
use assembly_xml::localization::load_locale;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use hyper::server::Server;
use mapr::Mmap;
use middleware::CorsLayerExt;
use paradox_typed_db::TypedDatabase;
use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
};
use tower::{make::Shared, ServiceBuilder};
use tower_http::{
    auth::RequireAuthorizationLayer, cors::CorsLayer, services::ServeDir, trace::TraceLayer,
};
use tracing::log::LevelFilter;

mod api;
mod auth;
mod config;
mod data;
mod middleware;
mod services;

fn load_db(path: &Path) -> color_eyre::Result<Database<'static>> {
    // Load the database file
    let file = File::open(path)
        .wrap_err_with(|| format!("Failed to open input file '{}'", path.display()))?;

    // Load the database
    let mmap = unsafe { Mmap::map(&file)? };
    // We want to keep this mapped until the end of the program!
    let mref: &'static Mmap = Box::leak(Box::new(mmap));
    let buf: &'static [u8] = mref.as_ref();
    Ok(Database::new(buf))
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    pretty_env_logger::formatted_builder()
        .filter_level(LevelFilter::Info)
        .init();

    color_eyre::install()?;
    let opts = Options::parse();

    // Load the config
    let cfg_path = opts.cfg;
    let cfg_file = fs::read_to_string(&cfg_path)
        .wrap_err_with(|| format!("Failed to open config file '{}'", cfg_path.display()))?;
    let cfg: Config = toml::from_str(&cfg_file)?;

    // Load the database
    let db = load_db(&cfg.data.cdclient)?;

    // Load the locale
    let locale_root = load_locale(&cfg.data.locale).context("Failed to load locale.xml")?;
    let lr = Arc::new(locale_root);

    // Load the typed database
    let tables = db.tables().unwrap();
    let canonical_base_url = cfg.general.base_url();

    let lu_res = cfg
        .data
        .lu_res_prefix
        .clone()
        .unwrap_or_else(|| format!("{}{}", canonical_base_url, router::RES_PREFIX));
    let res = LuRes::new(&lu_res);

    let tydb = TypedDatabase::new(tables)?;
    let tydb = Box::leak(Box::new(tydb));
    let rev = Box::leak(Box::new(ReverseLookup::new(tydb)));

    let api = api::service(
        &cfg.data,
        lr.clone(),
        AuthKind::of(&cfg.auth),
        canonical_base_url,
        db,
        tydb,
        rev,
    )?;

    // The 'new' fallback service
    let fallback = FallbackService::new(cfg.data.lu_json_cache.as_path());

    let app = services::app(
        &cfg.data.explorer_spa,
        tydb,
        LocaleRoot::new(lr),
        res,
        &cfg.general.domain,
    )?;

    // Initialize the lu-res cache
    let res = ServeDir::new(&cfg.data.lu_res_cache);

    let service = ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::configure(&cfg.general.cors))
        .layer(RedirectLayer::new(&cfg))
        .layer(PublicOrLayer::new(&cfg.data.public))
        .layer(RequireAuthorizationLayer::custom(Authorize::new(&cfg.auth)))
        .service(BaseRouter::new(api, app, res, fallback));

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

    // Finally, run the server
    Server::bind(&cfg.general.addr())
        .serve(Shared::new(service))
        .await
        .expect("server error");

    Ok(())
}
