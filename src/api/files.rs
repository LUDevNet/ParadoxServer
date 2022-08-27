use assembly_pack::pki::core::PackFileRef;
use serde::Serialize;
use std::{
    fmt, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::data::fs::{Loader, Node};

#[derive(Serialize)]
pub(crate) struct CRCReply<'a> {
    fs: Option<&'a Node>,
    pk: Option<&'a PackFileRef>,
}

#[derive(Clone)]
pub(crate) struct PackService {
    inner: Arc<Loader>,
}

#[derive(Debug)]
pub(crate) struct Error {
    inner: io::Error,
    path: PathBuf,
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to load PKI file at '{}'", self.path.display())
    }
}

impl PackService {
    pub fn new(res_path: &Path, pki_path: Option<&Path>) -> Result<Self, Error> {
        let mut loader = Loader::new();
        loader.load_dir(Path::new("client/res"), res_path);
        tracing::info!("PKI Path: {:?}", pki_path);
        if let Some(pki_path) = pki_path {
            loader.load_pki(pki_path).map_err(|inner| Error {
                inner,
                path: pki_path.to_owned(),
            })?;
        }

        let inner = Arc::new(loader);
        Ok(Self { inner })
    }

    pub fn lookup(&self, crc: u32) -> CRCReply {
        let loader = self.inner.as_ref();
        let fs = loader.get(crc).map(|e| &e.public);
        let pk = loader.get_pki(crc);
        CRCReply { fs, pk }
    }
}
