use std::{collections::BTreeMap, path::PathBuf};

use serde::Deserialize;
use structopt::StructOpt;

fn default_port() -> u16 {
    3030
}

fn default_lu_res_cache() -> PathBuf {
    PathBuf::from("lu-res")
}

#[derive(Deserialize)]
pub struct CorsOptions {
    pub all: bool,
    pub domains: Vec<String>,
}

impl Default for CorsOptions {
    fn default() -> Self {
        Self {
            all: true,
            domains: vec![],
        }
    }
}

#[derive(Deserialize)]
pub struct GeneralOptions {
    /// The port for the server
    #[serde(default = "default_port")]
    pub port: u16,
    /// Bind to `0.0.0.0` instead of `127.0.0.1`
    pub public: bool,
    /// The allowed cross-origin domains
    #[serde(default)]
    pub cors: CorsOptions,
    /// The base of the path
    pub base: Option<String>,
    /// The canonical domain
    pub domain: String,
    /// Whether this is served via https
    #[serde(default = "no")]
    pub secure: bool,
}

fn no() -> bool {
    false
}

#[derive(Deserialize)]
pub struct TlsOptions {
    /// Whether TLS is enabled
    pub enabled: bool,
    /// The private key file
    pub key: PathBuf,
    /// The certificate file
    pub cert: PathBuf,
}

#[derive(Deserialize)]
pub struct DataOptions {
    /// The CDClient database FDB file
    pub cdclient: PathBuf,
    /// The lu-explorer static files
    pub explorer_spa: PathBuf,
    /// The lu-res cache path
    #[serde(default = "default_lu_res_cache")]
    pub lu_res_cache: PathBuf,
    /// The LU-Res prefix
    pub lu_res_prefix: Option<String>,
    /// The locale.xml file
    pub locale: PathBuf,
}

#[derive(Deserialize)]
pub struct Config {
    pub general: GeneralOptions,
    pub tls: Option<TlsOptions>,
    pub data: DataOptions,
    #[serde(default)]
    pub host: Vec<HostConfig>,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Deserialize)]
pub struct HostConfig {
    pub name: String,
    #[serde(default)]
    pub redirect: bool,
    pub base: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    pub basic: Option<BTreeMap<String, String>>,
}

#[derive(StructOpt)]
/// Starts the server that serves a JSON API to the client files
pub struct Options {
    #[structopt(long, default_value = "paradox.toml")]
    pub cfg: PathBuf,
}
