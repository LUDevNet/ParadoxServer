use std::path::PathBuf;

use tracing::info;
use warp::{filters::BoxedFilter, fs::File, Filter};

pub fn make_fallback(lu_json_path: PathBuf) -> BoxedFilter<(File,)> {
    let maps_dir = lu_json_path.join("maps");
    info!("Maps on '{}'", maps_dir.display());
    let maps = warp::path("maps").and(warp::fs::dir(maps_dir));

    let scripts_dir = lu_json_path.join("scripts");
    info!("Scripts on '{}'", scripts_dir.display());
    let scripts = warp::path("scripts").and(warp::fs::dir(scripts_dir));

    warp::path("v0").and(maps.or(scripts).unify()).boxed()
}
