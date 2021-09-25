use std::path::PathBuf;

use serde::Deserialize;
use structopt::StructOpt;

fn default_port() -> u16 {
    3030
}

fn default_lu_res() -> String {
    String::from("https://xiphoseer.de/lu-res")
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
    /// The LU-Res prefix
    #[serde(default = "default_lu_res")]
    pub lu_res_prefix: String,
    /// The locale.xml file
    pub locale: PathBuf,
}

#[derive(Deserialize)]
pub struct Config {
    pub general: GeneralOptions,
    pub tls: Option<TlsOptions>,
    pub data: DataOptions,
}

#[derive(StructOpt)]
/// Starts the server that serves a JSON API to the client files
pub struct Options {
    #[structopt(long, default_value = "paradox.toml")]
    pub cfg: PathBuf,
}
