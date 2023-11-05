use std::{borrow::Cow, collections::BTreeMap, net::SocketAddr, path::PathBuf};

use clap::Parser;
use http::{header::InvalidHeaderValue, HeaderValue};
use serde::{
    de::{SeqAccess, Unexpected, Visitor},
    Deserialize, Deserializer,
};

fn default_port() -> u16 {
    3030
}

fn default_lu_res_cache() -> PathBuf {
    PathBuf::from("lu-res")
}

fn default_lu_json_cache() -> PathBuf {
    PathBuf::from("lu-json")
}

fn default_public() -> PathBuf {
    PathBuf::from("public")
}

fn deserialize_header_value_vec<'de, D>(deserializer: D) -> Result<Vec<HeaderValue>, D::Error>
where
    D: Deserializer<'de>,
{
    struct TheVisitor;

    impl<'de> Visitor<'de> for TheVisitor {
        type Value = Vec<HeaderValue>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(formatter, "a sequence of header values")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut vector = Vec::new();
            while let Some(src) = seq.next_element::<Cow<'de, str>>()? {
                vector.push(HeaderValue::from_str(src.as_ref()).map_err(
                    |_: InvalidHeaderValue| {
                        <A::Error as serde::de::Error>::invalid_value(
                            Unexpected::Str(src.as_ref()),
                            &"only visible ASCII characters (32-127)",
                        )
                    },
                )?);
            }
            Ok(vector)
        }
    }

    deserializer.deserialize_seq(TheVisitor)
}

#[derive(Deserialize)]
pub struct CorsOptions {
    pub all: bool,
    #[serde(default, deserialize_with = "deserialize_header_value_vec")]
    pub domains: Vec<HeaderValue>,
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

impl GeneralOptions {
    pub fn scheme(&self) -> &'static str {
        match self.secure {
            true => "https",
            false => "http",
        }
    }

    pub fn ip(&self) -> [u8; 4] {
        match self.public {
            true => [0, 0, 0, 0],
            false => [127, 0, 0, 1],
        }
    }

    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from((self.ip(), self.port))
    }

    /// Get the canonical base URL (without a trailing slash)
    pub fn base_url(&self) -> String {
        let mut start = self.scheme().to_string() + "://" + &self.domain;
        if let Some(b) = self.base.as_deref() {
            start.push('/');
            start.push_str(b);
        }
        start
    }
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
    /// The public directory
    #[serde(default = "default_public")]
    pub public: PathBuf,
    /// The `client/res` directory
    pub res: Option<PathBuf>,
    /// The `versions` directory
    pub versions: Option<PathBuf>,
    /// The CDClient database FDB file
    pub cdclient: PathBuf,
    /// The lu-explorer static files
    pub explorer_spa: PathBuf,
    /// The lu-res cache path
    #[serde(default = "default_lu_res_cache")]
    pub lu_res_cache: PathBuf,
    /// The lu-json cache path
    #[serde(default = "default_lu_json_cache")]
    pub lu_json_cache: PathBuf,
    /// The LU-Res prefix
    pub lu_res_prefix: Option<String>,
    /// The locale.xml file
    pub locale: PathBuf,
    /// The sqlite file to serve SQL queries from
    pub sqlite: PathBuf,
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
    #[serde(default)]
    pub user_agents: Vec<String>,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

#[derive(Parser)]
/// Starts the server that serves a JSON API to the client files
pub struct Options {
    #[clap(long, default_value = "paradox.toml")]
    pub cfg: PathBuf,
}
