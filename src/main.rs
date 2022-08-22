use std::{
    fs::File,
    io,
    net::SocketAddr,
    path::Path,
    str::FromStr,
    sync::{Arc, RwLock},
};

use api::{ApiFactory, ApiService};
use assembly_fdb::mem::Database;
use assembly_xml::localization::load_locale;
use auth::Authorize;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use config::{Config, Options};
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::{
    body::{Bytes, HttpBody},
    server::Server,
};
use mapr::Mmap;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use paradox_typed_db::TypedDatabase;
use services::BaseRouter;
use tokio::runtime::Handle;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{auth::RequireAuthorization, services::ServeDir};
use tracing::log::LevelFilter;
use warp::{hyper::Uri, Filter};

mod api;
mod auth;
mod config;
mod data;
mod fallback;
mod redirect;
mod services;
mod template;

use crate::{
    api::rev::ReverseLookup,
    auth::AuthKind,
    config::AuthConfig,
    data::{fs::LuRes, locale::LocaleRoot},
    fallback::make_fallback,
    template::{load_meta_template, FsEventHandler, TemplateUpdateTask},
};

fn new_io_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

fn response_to_boxed_error_io<B: http_body::Body<Data = Bytes> + Send + 'static>(
    r: Response<B>,
) -> Response<ResponseBody>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    r.map(|b| HttpBody::map_err(b, new_io_error).boxed_unsync())
}

type ResponseBody = UnsyncBoxBody<Bytes, io::Error>;

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

    let openapi = api::docs::OpenApiService::new(&api_url, auth_kind)?;
    let pack = api::files::PackService::new(res_path, pki_path.as_deref())?;
    let api_routes = ApiFactory {
        tydb,
        rev,
        lr: lr.clone(),
    }
    .make_api();
    let api = warp::path("api").and(
        fallback_routes
            .or(file_routes)
            .or(api_routes)
            .with(warp::compression::gzip()),
    );
    let api = ApiService::new(db, lr.clone(), pack, openapi, api_uri);

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

    // Set up the application
    let spa = ServeDir::new(spa_path)
        .append_index_html_on_directories(false)
        .fallback(template::SpaDynamic::new(
            tydb,
            LocaleRoot::new(lr),
            res,
            hb,
            &cfg.general.domain,
        ));

    // Initialize the lu-res cache
    let res = ServeDir::new(&cfg.data.lu_res_cache);

    // Finally collect all routes
    let routes = BaseRouter::new(api, spa, res);
    let protected = RequireAuthorization::custom(routes, Authorize::new(&cfg.auth));
    let public = services::PublicOr::new(protected, &cfg.data.public);
    let public = redirect::RedirectService::new(public, &cfg);

    // FIXME: Log middleware
    //let log = warp::log("paradox::routes");
    //let routes = routes.with(log);

    // FIXME: CORS middleware
    /*let mut cors = warp::cors();
    let cors_cfg = &cfg.general.cors;
    if cors_cfg.all {
        cors = cors.allow_any_origin();
    } else {
        for key in &cors_cfg.domains {
            cors = cors.allow_origin(key.as_ref());
        }
    }*/

    /*cors = cors
        .allow_methods(vec!["OPTIONS", "GET"])
        .allow_headers(vec!["authorization"]);
    let to_serve = routes.with(cors);*/
    //let server = warp::serve(to_serve);

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

    //server.run((ip, cfg.general.port)).await;
    let service = ServiceBuilder::new().service(public);

    // And run our service using `hyper`
    let addr = SocketAddr::from((ip, cfg.general.port));
    Server::bind(&addr)
        .serve(Shared::new(service))
        .await
        .expect("server error");

    Ok(())
}
