use assembly_fdb::mem::Database;
use assembly_xml::localization::load_locale;
use clap::Parser;
use color_eyre::eyre::{eyre, WrapErr};
use hyper::server::Server;
use mapr::Mmap;
use paradox_server::{
    api::{self, rev::ReverseLookup},
    auth::{AuthKind, Authorize},
    config::{Config, Options},
    data::locale::LocaleRoot,
    middleware::{CorsLayerExt, PublicOrLayer, RedirectLayer},
    services::{self, BaseRouter, FallbackService},
};
use paradox_typed_db::TypedDatabase;
use std::{
    fs::{self, File},
    path::Path,
};
use tower::{make::Shared, ServiceBuilder};
use tower_http::{
    auth::RequireAuthorizationLayer, cors::CorsLayer, services::ServeDir, trace::TraceLayer,
};
use tracing::log::LevelFilter;

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
    let locale_root = load_locale(&cfg.data.locale)
        .context("Failed to load locale.xml")
        .map(LocaleRoot::new)?;

    // Load the typed database
    let tables = db.tables().unwrap();
    let tydb = TypedDatabase::new(tables)?;
    let tydb = Box::leak(Box::new(tydb));
    let rev = Box::leak(Box::new(ReverseLookup::new(tydb)));

    // Set up res connection
    let base_url = cfg.general.base_url();

    // Initialize the Application
    let app = services::app(&cfg.data, tydb, locale_root.clone(), &base_url)?;

    // Initialize the Api
    let auth_kind = AuthKind::of(&cfg.auth);
    let api = api::service(&cfg.data, locale_root, auth_kind, base_url, db, tydb, rev)?;
    // Unfortunately still need the API fallback
    let api_fallback = FallbackService::new(cfg.data.lu_json_cache.as_path());

    // Initialize the lu-res cache
    let res = ServeDir::new(&cfg.data.lu_res_cache);

    let service = ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::configure(&cfg.general.cors))
        .layer(RedirectLayer::new(&cfg))
        .layer(PublicOrLayer::new(&cfg.data.public))
        .layer(RequireAuthorizationLayer::custom(Authorize::new(&cfg.auth)))
        .service(BaseRouter::new(api, app, res, api_fallback));

    // FIXME: TLS
    if let Some(tls_cfg) = cfg.tls {
        if tls_cfg.enabled {
            /*server
            .tls()
            .key_path(tls_cfg.key)
            .cert_path(tls_cfg.cert)
            .run((ip, cfg.general.port))
            .await;*/
            return Err(eyre!(
                "TLS support is currently unavailable, please use a proxy such as nginx or apache"
            ));
        }
    }

    // Finally, run the server
    Server::bind(&cfg.general.addr())
        .serve(Shared::new(service))
        .await
        .expect("server error");

    Ok(())
}
