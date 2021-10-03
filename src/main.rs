use std::{borrow::Cow, fs::File, sync::Arc};

use api::make_api;
use assembly_data::{fdb::mem::Database, xml::localization::load_locale};
use color_eyre::eyre::WrapErr;
use config::{Config, Options};
use handlebars::Handlebars;
use mapr::Mmap;
use paradox_typed_db::TypedDatabase;
use regex::{Captures, Regex};
use structopt::StructOpt;
use template::make_spa_dynamic;
use tracing::info;
use warp::{filters::BoxedFilter, hyper::Uri, path::Tail, Filter};

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
    let locale_root = load_locale(&cfg.data.locale)?;
    let lr = Arc::new(locale_root);

    // Load the typed database
    let tables = db.tables().unwrap();
    let lu_res_prefix = Box::leak(cfg.data.lu_res_prefix.clone().into_boxed_str());
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

    let routes = root.and(api.or(spa)).boxed();

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
