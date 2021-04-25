use std::{borrow::Cow, fs::File, sync::Arc};

use api::make_api;
use assembly_data::{fdb::mem::Database, xml::localization::load_locale};
use color_eyre::eyre::WrapErr;
use config::{Config, Options};
use handlebars::Handlebars;
use mapr::Mmap;
use regex::{Captures, Regex};
use structopt::StructOpt;
use template::make_spa_dynamic;
use warp::Filter;

mod api;
mod config;
mod template;

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

    let base = warp::path(cfg.general.base);
    let api = warp::path("api").and(make_api(db, lr.clone()));

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

    let lu_res_prefix = Box::leak(cfg.data.lu_res_prefix.clone().into_boxed_str());
    let spa_dynamic = make_spa_dynamic(lu_res_prefix, lr, db, hb);

    //let spa_file = warp::fs::file(spa_index);
    let spa = warp::fs::dir(spa_path).or(spa_dynamic);

    let routes = warp::get().and(base).and(api.or(spa));

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
