use std::{
    collections::BTreeMap,
    convert::TryFrom,
    ffi::OsStr,
    fs::{DirEntry, File},
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
};

use assembly_fdb::common::Latin1Str;
use assembly_pack::{
    crc::calculate_crc,
    pki::core::{PackFileRef, PackIndexFile},
};

use hyper::body::Bytes;
use serde::Serialize;
use tokio::sync::oneshot::Sender;
use tracing::error;

pub fn cleanup_path(url: &Latin1Str) -> Option<PathBuf> {
    let url = url.decode().replace('\\', "/").to_ascii_lowercase();
    let p = Path::new(&url);

    let mut path = Path::new("/textures/ui").to_owned();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            Component::Normal(seg) => path.push(seg),
            Component::RootDir => return None,
            Component::Prefix(_) => return None,
        }
    }
    path.set_extension("png");
    Some(path)
}

#[derive(Debug, Clone)]
pub struct LuRes {
    prefix: String,
}

impl LuRes {
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
        }
    }

    pub fn to_res_href(&self, path: &Path) -> String {
        format!("{}{}", self.prefix, path.display())
    }
}

pub enum Event {
    Path(Bytes, Sender<Reply>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    crc: u32,
}

#[derive(Debug, Clone)]
pub struct EventSender(tokio::sync::mpsc::Sender<Event>);

impl EventSender {
    pub async fn request(&self, tail: Bytes) -> Result<Reply, ()> {
        let (otx, orx) = tokio::sync::oneshot::channel();
        match self.0.send(Event::Path(tail, otx)).await {
            Ok(()) => match orx.await {
                Ok(v) => Ok(v),
                Err(e) => {
                    error!("{}", e);
                    Err(())
                }
            },
            Err(e) => {
                error!("{}", e);
                Err(())
            }
        }
    }
}

pub fn spawn_handler(_path: &Path) -> EventSender {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                None => break,
                Some(Event::Path(tail, reply)) => {
                    const PREFIX: &[u8] = br"client\res\";
                    let mut path = Vec::with_capacity(PREFIX.len() + tail.len());
                    path.extend_from_slice(PREFIX);
                    path.extend_from_slice(&tail);
                    let crc = calculate_crc(&path);

                    let t = Reply { crc };
                    // Ignore replies that get dropped
                    let _ = reply.send(t);
                }
            }
        }
    });
    EventSender(tx)
}

/// A single file
#[derive(Debug, Copy, Clone, Serialize)]
pub enum NodeKind {
    ZoneFile,
    LevelFile,
    DirectDrawSurface,
    Script,
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Server side path: DO NOT SERIALIZE
    pub rel_path: PathBuf,
    /// The kind of this file
    pub kind: NodeKind,
}

pub struct ServerNode {
    pub public: Node,
    /// server path, DO NOT SERIALIZE
    pub abs_path: PathBuf,
}

pub struct Loader {
    /// Maps path CRCs to a node
    entries: BTreeMap<u32, ServerNode>,
    /// The data from a pack index file
    pki: PackIndexFile,
}

impl Loader {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            pki: PackIndexFile {
                archives: Vec::new(),
                files: BTreeMap::new(),
            },
        }
    }

    pub fn get(&self, crc: u32) -> Option<&ServerNode> {
        self.entries.get(&crc)
    }

    pub fn get_pki(&self, crc: u32) -> Option<&PackFileRef> {
        self.pki
            .files
            .get(&crc)
            .map(|r| &self.pki.archives[r.pack_file as usize])
    }

    fn error(&mut self, path: &Path, error: io::Error) {
        error!("{} {}", path.display(), error)
    }

    pub fn load_pki(&mut self, path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        self.pki = PackIndexFile::try_from(file)
            .map_err(|error| io::Error::new(ErrorKind::Other, error))?;
        Ok(())
    }

    /*pub fn load_luz(&mut self, path: &Path) -> Option<ZoneFile<ZonePaths>> {
        let file = File::open(&path).map_err(|e| self.error(path, e)).ok()?;
        let mut buf = BufReader::new(file);
        match match ZoneFile::try_from_luz(&mut buf) {
            Ok(zf) => zf.parse_paths(),
            Err(e) => {
                self.error(path, io::Error::new(ErrorKind::Other, e));
                return None;
            }
        } {
            Ok(zf) => Some(zf),
            Err(_e) => {
                /* TODO */
                None
            }
        }
    }*/

    pub fn load_node(&mut self, rel_parent: &Path, entry: DirEntry) {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
        if path.is_dir() {
            let relative = rel_parent.join(&name);
            self.load_dir(&relative, &path);
        }
        if path.is_file() {
            let ext = path.extension().and_then(OsStr::to_str);
            if let Some(kind) = match ext {
                Some("luz") => Some(NodeKind::ZoneFile),
                Some("lvl") => Some(NodeKind::LevelFile),
                Some("dds") => Some(NodeKind::DirectDrawSurface),
                Some("lua") => Some(NodeKind::Script),
                _ => None,
            } {
                let rel_path = rel_parent.join(&name);
                let crc = calculate_crc(rel_path.to_string_lossy().as_bytes());
                self.entries.insert(
                    crc,
                    ServerNode {
                        abs_path: path,
                        public: Node { rel_path, kind },
                    },
                );
            }
        }
    }

    pub fn load_dir(&mut self, relative: &Path, absolute: &Path) {
        match std::fs::read_dir(absolute) {
            Ok(read_dir) => {
                for entry in read_dir {
                    match entry {
                        Ok(entry) => self.load_node(relative, entry),
                        Err(e) => {
                            self.error(absolute, e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => self.error(absolute, e),
        }
    }
}
