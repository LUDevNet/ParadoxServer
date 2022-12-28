use std::{
    fmt, io,
    sync::{Arc, RwLock},
};

pub mod router;
use paradox_typed_db::TypedDatabase;
pub use router::BaseRouter;
mod fallback;
pub use fallback::FallbackService;
use tower_http::services::ServeDir;
mod template;
pub use template::SpaDynamic;

use crate::{
    config::DataOptions,
    data::{fs::LuRes, locale::LocaleRoot},
};

#[derive(Debug)]
pub enum Error {
    Hyper(hyper::Error),
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Self::Hyper(e)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Hyper(e) => Some(e),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Hyper(e) => fmt::Display::fmt(e, f),
        }
    }
}

pub fn app(
    cfg: &DataOptions,
    tydb: &'static TypedDatabase<'static>,
    locale_root: LocaleRoot,
    base_url: &str,
) -> Result<ServeDir<SpaDynamic>, color_eyre::Report> {
    let spa_path = &cfg.explorer_spa;
    let spa_index = spa_path.join("index.html");

    // Create handlebars registry
    let hb = Arc::new(RwLock::new(template::Template::new()));
    template::load_meta_template(&hb, &spa_index)?;
    template::spawn_watcher(&spa_index, hb.clone())?;

    // Set up the application
    let res = LuRes::new(
        cfg.lu_res_prefix
            .clone()
            .unwrap_or_else(|| base_url.to_string() + router::RES_PREFIX),
    );
    let spa_dynamic = template::SpaDynamic::new(tydb, locale_root, res, hb, base_url);
    Ok(ServeDir::new(spa_path)
        .append_index_html_on_directories(false)
        .fallback(spa_dynamic))
}
