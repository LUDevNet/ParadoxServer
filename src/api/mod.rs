use std::{
    borrow::Borrow,
    convert::Infallible,
    error::Error,
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
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter, Reply,
};

use crate::data::locale::LocaleRoot;

use self::{
    docs::OpenApiService,
    files::PackService,
    rev::{make_api_rev, ReverseLookup},
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

fn map_res<E: Error>(v: Result<Json, E>) -> WithStatus<Json> {
    match v {
        Ok(res) => wrap_200(res),
        Err(e) => wrap_500(warp::reply::json(&e.to_string())),
    }
}

fn map_opt_res<E: Error>(v: Result<Option<Json>, E>) -> WithStatus<Json> {
    match v {
        Ok(Some(res)) => wrap_200(res),
        Ok(None) => wrap_404(warp::reply::json(&())),
        Err(e) => wrap_500(warp::reply::json(&e.to_string())),
    }
}

fn map_opt(v: Option<Json>) -> WithStatus<Json> {
    match v {
        Some(res) => wrap_200(res),
        None => wrap_404(warp::reply::json(&())),
    }
}

fn wrap_404<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND)
}

pub fn wrap_200<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::OK)
}

pub fn wrap_500<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::INTERNAL_SERVER_ERROR)
}

fn make_api_catch_all() -> impl Filter<Extract = (WithStatus<Json>,), Error = Infallible> + Clone {
    warp::any().map(|| warp::reply::json(&404)).map(wrap_404)
}

fn tydb_filter<'db>(
    db: &'db TypedDatabase<'db>,
) -> impl Filter<Extract = (&'db TypedDatabase<'db>,), Error = Infallible> + Clone + 'db {
    warp::any().map(move || db)
}

pub(crate) struct ApiFactory {
    pub tydb: &'static TypedDatabase<'static>,
    pub rev: &'static ReverseLookup,
    pub lr: Arc<LocaleNode>,
}

impl ApiFactory {
    pub(crate) fn make_api(self) -> BoxedFilter<(WithStatus<Json>,)> {
        let loc = LocaleRoot::new(self.lr.clone());

        let v0_base = warp::path("v0");

        let v0_rev = warp::path("rev").and(make_api_rev(self.tydb, loc, self.rev));
        let v0 = v0_base.and(v0_rev);
        let catch_all = make_api_catch_all();

        v0.or(catch_all).unify().boxed()
    }
}

enum ApiRoute<'r> {
    Tables,
    TableByName(&'r str),
    AllTableRows(&'r str),
    TableRowsByPK(&'r str, &'r str),
    Locale(Split<'r, char>),
    Crc(u32),
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
        let mut parts = s.trim_start_matches('/').split('/');
        match parts.next() {
            Some("v0") => Self::v0(parts),
            Some("v1") => Self::v1(parts),
            Some("") => match parts.next() {
                None => Ok(Self::SwaggerUI),
                _ => Err(()),
            },
            None => Ok(Self::SwaggerUIRedirect),
            _ => Err(()),
        }
    }
}

enum Accept {
    Json,
    Yaml,
}

#[derive(Clone)]
pub struct ApiService {
    pub db: Database<'static>,
    pub locale_root: Arc<LocaleNode>,
    pub openapi: OpenApiService,
    pack: files::PackService,
    api_url: HeaderValue,
}

fn into_other_io_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
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
    ) -> Self {
        let api_url = HeaderValue::from_str(&api_uri.to_string()).unwrap();
        Self {
            pack,
            db,
            locale_root,
            openapi,
            api_url,
        }
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

    fn reply<T: Serialize>(
        accept: Accept,
        v: &T,
    ) -> Result<http::Response<hyper::Body>, io::Error> {
        match accept {
            Accept::Json => Self::reply_json(v),
            Accept::Yaml => Self::reply_yaml(v),
        }
    }

    fn reply_json<T: Serialize>(v: &T) -> Result<http::Response<hyper::Body>, io::Error> {
        let body = serde_json::to_string(&v).map_err(into_other_io_error)?;
        Ok(Self::reply_string(body, APPLICATION_JSON))
    }

    fn reply_yaml<T: Serialize>(v: &T) -> Result<http::Response<hyper::Body>, io::Error> {
        let body = serde_yaml::to_string(&v).map_err(into_other_io_error)?;
        Ok(Self::reply_string(body, APPLICATION_YAML))
    }

    fn reply_404() -> http::Response<hyper::Body> {
        let mut r = Response::new(hyper::Body::from("404"));
        *r.status_mut() = http::StatusCode::NOT_FOUND;

        let content_length = HeaderValue::from(3);
        r.headers_mut().append(CONTENT_LENGTH, content_length);
        r.headers_mut().append(CONTENT_TYPE, APPLICATION_JSON);
        r
    }

    fn db_api<T: Serialize>(
        &self,
        accept: Accept,
        f: impl FnOnce(Database<'static>) -> Result<T, CastError>,
    ) -> Result<Response<hyper::Body>, io::Error> {
        let v = f(self.db).map_err(into_other_io_error)?;
        match accept {
            Accept::Json => Self::reply_json(&v),
            Accept::Yaml => Self::reply_yaml(&v),
        }
    }

    /// Get data from `locale.xml`
    fn locale(
        &self,
        accept: Accept,
        rest: Split<char>,
    ) -> Result<Response<hyper::Body>, io::Error> {
        match locale::select_node(self.locale_root.as_ref(), rest) {
            Some((node, locale::Mode::All)) => Self::reply(accept, &locale::All::new(node)),
            Some((node, locale::Mode::Pod)) => Self::reply(accept, &locale::Pod::new(node)),
            None => Ok(Self::reply_404()),
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
        let response = match ApiRoute::from_str(req.uri().path()) {
            Ok(ApiRoute::Tables) => self.db_api(accept, tables::tables_json),
            Ok(ApiRoute::TableByName(name)) => {
                self.db_api(accept, |db| tables::table_def_json(db, name))
            }
            Ok(ApiRoute::AllTableRows(name)) => {
                self.db_api(accept, |db| tables::table_all_json(db, name))
            }
            Ok(ApiRoute::TableRowsByPK(name, key)) => {
                self.db_api(accept, |db| tables::table_key_json(db, name, key))
            }
            Ok(ApiRoute::Locale(rest)) => self.locale(accept, rest),
            Ok(ApiRoute::OpenApiV0) => Self::reply_json(self.openapi.as_ref()),
            Ok(ApiRoute::SwaggerUI) => Ok(Self::reply_static(SWAGGER_UI_HTML)),
            Ok(ApiRoute::SwaggerUIRedirect) => self.swagger_ui_redirect(),
            Ok(ApiRoute::Crc(crc)) => Self::reply(accept, &self.pack.lookup(crc)),
            Err(()) => Ok(Self::reply_404()),
        };
        std::future::ready(response)
    }
}
