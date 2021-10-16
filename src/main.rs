use std::{
    fs::File,
    sync::{Arc, RwLock},
};

use api::make_api;
use assembly_data::{fdb::mem::Database, xml::localization::load_locale};
use color_eyre::eyre::WrapErr;
use config::{Config, Options};
use handlebars::Handlebars;
use mapr::Mmap;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use paradox_typed_db::TypedDatabase;
use structopt::StructOpt;
use template::make_spa_dynamic;
use tokio::runtime::Handle;
use warp::{filters::BoxedFilter, Filter};

mod api;
mod auth;
mod config;
mod data;
mod fallback;
mod redirect;
mod template;

use crate::{
    api::rev::ReverseLookup,
    fallback::make_fallback,
    redirect::{add_host_filters, add_redirect_filters, base_filter},
    template::{load_meta_template, FsEventHandler, TemplateUpdateTask},
};

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
    let lu_res = cfg.data.lu_res_prefix.clone().unwrap_or_else(|| {
        if let Some(b) = cfg_g.base.as_deref() {
            format!("{}://{}/{}/lu-res", scheme, &cfg_g.domain, b)
        } else {
            format!("{}://{}/lu-res", scheme, &cfg_g.domain)
        }
    });

    let lu_res_prefix = Box::leak(lu_res.clone().into_boxed_str());
    let tydb = TypedDatabase::new(lr.clone(), lu_res_prefix, tables)?;
    let data = Box::leak(Box::new(tydb));
    let rev = Box::leak(Box::new(ReverseLookup::new(data)));

    // Make the API
    let lu_json_path = cfg.data.lu_json_cache.clone();
    let fallback_routes = make_fallback(lu_json_path);

    let api_routes = make_api(db, data, rev, lr.clone());
    let api = warp::path("api").and(fallback_routes.or(api_routes));

    let spa_path = &cfg.data.explorer_spa;
    let spa_index = spa_path.join("index.html");

    // Create handlebars registry
    let hb = Arc::new(RwLock::new(Handlebars::new()));

    load_meta_template(&hb, &spa_index)?;

    // Setup the watcher
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let eh = FsEventHandler::new(tx);
    let mut watcher = recommended_watcher(eh)?;
    watcher.watch(&spa_index, RecursiveMode::Recursive).unwrap();

    let rt = Handle::current();

    rt.spawn(TemplateUpdateTask::new(rx, hb.clone()));

    let spa_dynamic = make_spa_dynamic(data, hb, &cfg.general.domain);

    //let spa_file = warp::fs::file(spa_index);
    let spa = warp::fs::dir(spa_path.clone()).or(spa_dynamic);

    // Initialize the lu-res cache
    let res = warp::path("lu-res")
        .and(warp::fs::dir(cfg.data.lu_res_cache.clone()))
        .boxed();

    // Finally collect all routes
    let routes = res.or(api).or(spa);

    let auth_fn = auth::make_auth_fn(&cfg.auth);

    let auth = warp::filters::header::optional::<String>("Authorization").and_then(auth_fn);

    let routes = auth.or(routes);

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
