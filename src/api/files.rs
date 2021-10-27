use assembly_pack::pki::core::PackFileRef;
use color_eyre::eyre::Context;
use serde::Serialize;
use std::{path::Path, sync::Arc};
use tracing::error;

use warp::{
    filters::BoxedFilter,
    hyper::StatusCode,
    reply::{json, with_status, Json, WithStatus},
    Filter,
};

use crate::data::fs::{Loader, Node};

#[derive(Serialize)]
struct CRCReply<'a> {
    fs: Option<&'a Node>,
    pk: Option<&'a PackFileRef>,
}

/// Lookup information on a CRC i.e. a file path in the client
pub fn make_crc_lookup_filter(
    res_path: &Path,
    pki_path: Option<&Path>,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let mut loader = Loader::new();
    loader.load_dir(Path::new("client/res"), res_path);
    if let Some(pki_path) = pki_path {
        if let Err(e) = loader
            .load_pki(pki_path)
            .with_context(|| format!("Failed to load PKI file at '{}'", pki_path.display()))
        {
            error!("{}", e);
        }
    }

    let loader = Arc::new(loader);

    warp::path::param()
        .map(move |crc: u32| {
            let fs = loader.get(crc).map(|e| &e.public);
            let pk = loader.get_pki(crc);
            with_status(json(&CRCReply { fs, pk }), StatusCode::OK)
        })
        .boxed()
}
