use std::{
    borrow::Borrow,
    future::Ready,
    io,
    str::{FromStr, Split, Utf8Error},
    sync::Arc,
    task::{self, Poll},
};

use assembly_core::buffer::CastError;
use assembly_fdb::mem::Database;
use assembly_xml::localization::LocaleNode;
use http::{
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    HeaderValue, Request, Response, StatusCode, Uri,
};
use paradox_typed_db::TypedDatabase;
use percent_encoding::percent_decode_str;
use serde::Serialize;
use tower::Service;

use crate::data::locale::LocaleRoot;

use self::{
    docs::OpenApiService,
    files::PackService,
    rev::{RevService, ReverseLookup},
};

pub mod adapter;
pub mod docs;
pub mod files;
mod locale;
pub mod rev;
pub mod tables;

#[derive(Clone, Debug)]
pub struct PercentDecoded(pub String);

impl FromStr for PercentDecoded {
    type Err = Utf8Error;
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = percent_decode_str(s).decode_utf8()?.to_string();
        Ok(PercentDecoded(s))
    }
}

impl Borrow<String> for PercentDecoded {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl ToString for PercentDecoded {
    #[inline]
    fn to_string(&self) -> String {
        self.0.clone()
    }
}

#[derive(Debug)]
enum ApiRoute<'r> {
    Tables,
    TableByName(&'r str),
    AllTableRows(&'r str),
    TableRowsByPK(&'r str, &'r str),
    Locale(Split<'r, char>),
    Crc(u32),
    Rev(rev::Route),
    OpenApiV0,
    SwaggerUI,
    SwaggerUIRedirect,
}

impl<'r> ApiRoute<'r> {
    fn v0(mut parts: Split<'r, char>) -> Result<Self, ()> {
        match parts.next() {
            Some("tables") => match parts.next() {
                None => Ok(Self::Tables),
                Some(name) => match parts.next() {
                    None => Ok(Self::TableByName(name)),
                    Some("def") => match parts.next() {
                        None => Ok(Self::TableByName(name)),
                        _ => Err(()),
                    },
                    Some("all") => match parts.next() {
                        None => Ok(Self::AllTableRows(name)),
                        _ => Err(()),
                    },
                    Some(key) => match parts.next() {
                        None => Ok(Self::TableRowsByPK(name, key)),
                        _ => Err(()),
                    },
                },
            },
            Some("locale") => Ok(Self::Locale(parts)),
            Some("rev") => rev::Route::from_parts(parts).map(ApiRoute::Rev),
            Some("crc") => match parts.next() {
                Some(crc) => match crc.parse() {
                    Ok(crc) => Ok(Self::Crc(crc)),
                    _ => Err(()),
                },
                _ => Err(()),
            },
            Some("openapi.json") => match parts.next() {
                None => Ok(Self::OpenApiV0),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }

    fn v1(mut parts: Split<'r, char>) -> Result<Self, ()> {
        match parts.next() {
            Some("tables") => match parts.next() {
                None => Ok(Self::Tables),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }

    fn from_str(s: &'r str) -> Result<Self, ()> {
        if s.is_empty() {
            return Ok(Self::SwaggerUIRedirect);
        }
        let mut parts = s.trim_start_matches('/').split('/');
        match parts.next() {
            Some("v0") => Self::v0(parts),
            Some("v1") => Self::v1(parts),
            Some("") => match parts.next() {
                None => Ok(Self::SwaggerUI),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}

enum Accept {
    Json,
    Yaml,
}

fn into_other_io_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

fn reply_static(body: &'static str) -> http::Response<hyper::Body> {
    let mut r = Response::new(hyper::Body::from(body));
    r.headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from(body.len()));
    r.headers_mut().append(CONTENT_TYPE, TEXT_HTML);
    r
}

fn reply_string(body: String, content_type: HeaderValue) -> http::Response<hyper::Body> {
    let is_404 = body == "null";
    let content_length = HeaderValue::from(body.len());
    let mut r = Response::new(hyper::Body::from(body));

    if is_404 {
        // FIXME: hack; handle T = Option<U> for 404 properly
        *r.status_mut() = http::StatusCode::NOT_FOUND;
    }
    r.headers_mut().append(CONTENT_LENGTH, content_length);
    r.headers_mut().append(CONTENT_TYPE, content_type);
    r
}

fn reply_opt<T: Serialize>(
    accept: Accept,
    v: Option<&T>,
) -> Result<http::Response<hyper::Body>, io::Error> {
    v.map(|v| reply(accept, v))
        .unwrap_or_else(|| Ok(reply_404()))
}

fn reply<T: Serialize>(accept: Accept, v: &T) -> Result<http::Response<hyper::Body>, io::Error> {
    match accept {
        Accept::Json => reply_json(v),
        Accept::Yaml => reply_yaml(v),
    }
}

fn reply_json<T: Serialize>(v: &T) -> Result<http::Response<hyper::Body>, io::Error> {
    let body = serde_json::to_string(&v).map_err(into_other_io_error)?;
    Ok(reply_string(body, APPLICATION_JSON))
}

fn reply_yaml<T: Serialize>(v: &T) -> Result<http::Response<hyper::Body>, io::Error> {
    let body = serde_yaml::to_string(&v).map_err(into_other_io_error)?;
    Ok(reply_string(body, APPLICATION_YAML))
}

fn reply_404() -> http::Response<hyper::Body> {
    let mut r = Response::new(hyper::Body::from("404"));
    *r.status_mut() = http::StatusCode::NOT_FOUND;

    let content_length = HeaderValue::from(3);
    r.headers_mut().append(CONTENT_LENGTH, content_length);
    r.headers_mut().append(CONTENT_TYPE, APPLICATION_JSON);
    r
}

#[derive(Clone)]
pub struct ApiService {
    pub db: Database<'static>,
    pub locale_root: Arc<LocaleNode>,
    pub openapi: OpenApiService,
    pack: files::PackService,
    api_url: HeaderValue,
    rev: rev::RevService,
}

#[allow(clippy::declare_interior_mutable_const)] // c.f. https://github.com/rust-lang/rust-clippy/issues/5812
const APPLICATION_JSON: HeaderValue = HeaderValue::from_static("application/json; charset=utf-8");
#[allow(clippy::declare_interior_mutable_const)]
const APPLICATION_YAML: HeaderValue = HeaderValue::from_static("application/yaml; charset=utf-8");
#[allow(clippy::declare_interior_mutable_const)]
const TEXT_HTML: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");

impl ApiService {
    pub(crate) fn new(
        db: Database<'static>,
        locale_root: Arc<LocaleNode>,
        pack: PackService,
        openapi: OpenApiService,
        api_uri: Uri,
        tydb: &'static TypedDatabase,
        rev: &'static ReverseLookup,
    ) -> Self {
        let api_url = HeaderValue::from_str(&api_uri.to_string()).unwrap();
        Self {
            pack,
            db,
            locale_root: locale_root.clone(),
            openapi,
            api_url,
            rev: RevService::new(tydb, LocaleRoot { root: locale_root }, rev),
        }
    }

    fn db_api<T: Serialize>(
        &self,
        accept: Accept,
        f: impl FnOnce(Database<'static>) -> Result<T, CastError>,
    ) -> Result<Response<hyper::Body>, io::Error> {
        let v = f(self.db).map_err(into_other_io_error)?;
        match accept {
            Accept::Json => reply_json(&v),
            Accept::Yaml => reply_yaml(&v),
        }
    }

    /// Get data from `locale.xml`
    fn locale(
        &self,
        accept: Accept,
        rest: Split<char>,
    ) -> Result<Response<hyper::Body>, io::Error> {
        match locale::select_node(self.locale_root.as_ref(), rest) {
            Some((node, locale::Mode::All)) => reply(accept, &locale::All::new(node)),
            Some((node, locale::Mode::Pod)) => reply(accept, &locale::Pod::new(node)),
            None => Ok(reply_404()),
        }
    }

    fn swagger_ui_redirect(&self) -> Result<http::Response<hyper::Body>, io::Error> {
        let mut r = http::Response::new(hyper::Body::empty());
        *r.status_mut() = StatusCode::PERMANENT_REDIRECT;
        r.headers_mut().append(LOCATION, self.api_url.clone());
        Ok(r)
    }
}

const SWAGGER_UI_HTML: &str = include_str!("../../res/api.html");

impl<ReqBody> Service<Request<ReqBody>> for ApiService {
    type Error = io::Error;
    type Response = Response<hyper::Body>;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// This is the main entry point to the API service.
    ///
    /// Here, we turn [ApiRoute]s into [http::Response]s
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let accept = match req.headers().get(ACCEPT) {
            Some(s) if s == "application/yaml" => Accept::Yaml,
            _ => Accept::Json,
        };
        let route = match ApiRoute::from_str(req.uri().path()) {
            Ok(route) => {
                tracing::info!("API Route: {:?}", route);
                route
            }
            Err(()) => return std::future::ready(Ok(reply_404())),
        };
        let response = match route {
            ApiRoute::Tables => self.db_api(accept, tables::tables_json),
            ApiRoute::TableByName(name) => {
                self.db_api(accept, |db| tables::table_def_json(db, name))
            }
            ApiRoute::AllTableRows(name) => {
                self.db_api(accept, |db| tables::table_all_json(db, name))
            }
            ApiRoute::TableRowsByPK(name, key) => {
                self.db_api(accept, |db| tables::table_key_json(db, name, key))
            }
            ApiRoute::Locale(rest) => self.locale(accept, rest),
            ApiRoute::OpenApiV0 => reply_json(self.openapi.as_ref()),
            ApiRoute::SwaggerUI => Ok(reply_static(SWAGGER_UI_HTML)),
            ApiRoute::SwaggerUIRedirect => self.swagger_ui_redirect(),
            ApiRoute::Crc(crc) => reply(accept, &self.pack.lookup(crc)),
            ApiRoute::Rev(route) => return self.rev.call((accept, route)),
        };
        std::future::ready(response)
    }
}
