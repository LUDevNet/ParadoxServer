use std::{
    borrow::Borrow,
    fmt,
    future::{ready, Ready},
    io,
    path::Path,
    str::{FromStr, Split, Utf8Error},
    task::{self, Poll},
};

use assembly_core::buffer::CastError;
use assembly_fdb::mem::Database;
use futures_util::{future::BoxFuture, Future, FutureExt};
use http::{
    header::{ACCEPT, ALLOW, CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    HeaderValue, Method, Request, Response, StatusCode, Uri,
};
use hyper::body::Bytes;
use paradox_typed_db::TypedDatabase;
use percent_encoding::percent_decode_str;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use tower::Service;

use crate::{
    auth::AuthKind,
    config::DataOptions,
    data::{
        fs::{spawn_handler, EventSender},
        locale::LocaleRoot,
    },
    services::router,
};

use self::{
    docs::OpenApiService,
    files::PackService,
    rev::{RevService, ReverseLookup},
};

pub mod adapter;
pub mod docs;
pub mod files;
pub mod graphql;
mod locale;
mod query;
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

impl Borrow<str> for PercentDecoded {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PercentDecoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// This enum is for server side errors (i.e. `5XX`) only!
pub enum ApiError {
    DB(CastError),
    Sqlite(rusqlite::Error),
    GraphQl(graphql::QueryError),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
}

pub type ApiResponse = Response<hyper::Body>;
pub type ApiResult = Result<ApiResponse, ApiError>;

impl From<CastError> for ApiError {
    fn from(value: CastError) -> Self {
        Self::DB(value)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<graphql::QueryError> for ApiError {
    fn from(value: graphql::QueryError) -> Self {
        Self::GraphQl(value)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<serde_yaml::Error> for ApiError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

impl From<ApiError> for io::Error {
    fn from(value: ApiError) -> Self {
        match value {
            ApiError::DB(e) => into_other_io_error(e),
            ApiError::Sqlite(e) => into_other_io_error(e),
            ApiError::GraphQl(e) => into_other_io_error(e),
            ApiError::Json(e) => into_other_io_error(e),
            ApiError::Yaml(e) => into_other_io_error(e),
        }
    }
}

pub struct RestPath<'r>(Split<'r, char>);

impl<'r> RestPath<'r> {
    pub fn join(self, ch: char) -> String {
        let mut key = String::new();
        for part in self.0 {
            if !key.is_empty() {
                key.push(ch);
            }
            key.push_str(part);
        }
        key
    }
}

impl<'r> fmt::Debug for RestPath<'r> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.clone()).finish()
    }
}

#[derive(Debug)]
enum ApiRoute<'r> {
    Tables,
    TableByName(&'r str),
    AllTableRows(&'r str),
    TableRowsByPK(&'r str, &'r str),
    Query(PercentDecoded),
    GraphQl(PercentDecoded),
    Locale(RestPath<'r>),
    Crc(u32),
    Rev(rev::Route),
    OpenApiV0,
    SwaggerUI,
    SwaggerUIRedirect,
    Res(RestPath<'r>),
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
            Some("query") => match parts.next() {
                Some(query) => Ok(Self::Query(
                    PercentDecoded::from_str(query).map_err(|_e| ())?,
                )),
                None => Err(()),
            },
            Some("graphql") => match parts.next() {
                Some(query) => Ok(Self::GraphQl(
                    PercentDecoded::from_str(query).map_err(|_e| ())?,
                )),
                None => Err(()),
            },
            Some("locale") => Ok(Self::Locale(RestPath(parts))),
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
            Some("res") => Ok(Self::Res(RestPath(parts))),
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

impl Accept {
    pub fn content_type(&self) -> HeaderValue {
        match self {
            Accept::Json => APPLICATION_JSON,
            Accept::Yaml => APPLICATION_YAML,
        }
    }
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

fn reply_string(
    body: String,
    content_type: HeaderValue,
    status: StatusCode,
) -> http::Response<hyper::Body> {
    let content_length = HeaderValue::from(body.len());
    let mut r = Response::new(hyper::Body::from(body));

    *r.status_mut() = status;
    r.headers_mut().append(CONTENT_LENGTH, content_length);
    r.headers_mut().append(CONTENT_TYPE, content_type);
    r
}

fn reply_opt<T: Serialize>(
    accept: Accept,
    v: Option<&T>,
) -> Result<http::Response<hyper::Body>, ApiError> {
    v.map(|v| reply(accept, v, StatusCode::OK))
        .unwrap_or_else(|| Ok(reply_404()))
}

fn reply<T: Serialize>(
    accept: Accept,
    v: &T,
    status: StatusCode,
) -> Result<http::Response<hyper::Body>, ApiError> {
    match accept {
        Accept::Json => reply_json(v, status),
        Accept::Yaml => reply_yaml(v, status),
    }
}

fn reply_json<T: Serialize>(
    v: &T,
    status: StatusCode,
) -> Result<http::Response<hyper::Body>, ApiError> {
    let body = serde_json::to_string(&v)?;
    Ok(reply_string(body, APPLICATION_JSON, status))
}

fn reply_yaml<T: Serialize>(
    v: &T,
    status: StatusCode,
) -> Result<http::Response<hyper::Body>, ApiError> {
    let body = serde_yaml::to_string(&v)?;
    Ok(reply_string(body, APPLICATION_YAML, status))
}

/// Reply with a `200 OK` without any content
fn reply_200(a: Accept) -> http::Response<hyper::Body> {
    let mut r = Response::new(hyper::Body::empty());
    *r.status_mut() = http::StatusCode::OK;
    r.headers_mut().append(CONTENT_TYPE, a.content_type());
    r
}

fn reply_404() -> http::Response<hyper::Body> {
    let mut r = Response::new(hyper::Body::from("404"));
    *r.status_mut() = http::StatusCode::NOT_FOUND;

    let content_length = HeaderValue::from(3);
    r.headers_mut().append(CONTENT_LENGTH, content_length);
    r.headers_mut().append(CONTENT_TYPE, APPLICATION_JSON);
    r
}

#[derive(Serialize)]
pub struct ErrorPayload {
    status: u16,
    error: &'static str,
    reason: String,
}

impl ErrorPayload {
    pub fn new<R: fmt::Display>(status: StatusCode, error: &'static str, reason: R) -> Self {
        Self {
            status: status.as_u16(),
            error,
            reason: reason.to_string(),
        }
    }
}

fn reply_400(accept: Accept, error: &'static str, reason: impl fmt::Display) -> ApiResult {
    let status = StatusCode::BAD_REQUEST;
    reply(accept, &ErrorPayload::new(status, error, reason), status)
}

fn reply_405(allow: &HeaderValue) -> http::Response<hyper::Body> {
    let mut r = Response::new(hyper::Body::from("405"));
    *r.status_mut() = http::StatusCode::METHOD_NOT_ALLOWED;

    let content_length = HeaderValue::from(3);
    r.headers_mut().append(ALLOW, allow.clone());
    r.headers_mut().append(CONTENT_LENGTH, content_length);
    r.headers_mut().append(CONTENT_TYPE, APPLICATION_JSON);
    r
}

#[derive(Clone)]
pub struct ApiService {
    pub db: Database<'static>,
    pub locale_root: LocaleRoot,
    pub openapi: OpenApiService,
    pack: files::PackService,
    api_url: HeaderValue,
    rev: rev::RevService,
    res: EventSender,
    sqlite_path: &'static Path,
    db_table_rels: &'static graphql::TableRels,
}

#[allow(clippy::declare_interior_mutable_const)] // c.f. https://github.com/rust-lang/rust-clippy/issues/5812
const APPLICATION_JSON: HeaderValue = HeaderValue::from_static("application/json; charset=utf-8");
#[allow(clippy::declare_interior_mutable_const)]
const APPLICATION_YAML: HeaderValue = HeaderValue::from_static("application/yaml; charset=utf-8");
#[allow(clippy::declare_interior_mutable_const)]
const TEXT_HTML: HeaderValue = HeaderValue::from_static("text/html; charset=utf-8");
#[allow(clippy::declare_interior_mutable_const)]
const TEXT_CSV: HeaderValue = HeaderValue::from_static("text/csv; charset=utf-8");

impl ApiService {
    #[allow(clippy::too_many_arguments)] // FIXME
    pub(crate) fn new(
        db: Database<'static>,
        locale_root: LocaleRoot,
        pack: PackService,
        openapi: OpenApiService,
        api_uri: Uri,
        tydb: &'static TypedDatabase,
        rev: &'static ReverseLookup,
        db_table_rels: &'static graphql::TableRels,
        res_path: &Path,
        sqlite_path: &'static Path,
    ) -> Self {
        let api_url = HeaderValue::from_str(&api_uri.to_string()).unwrap();
        Self {
            pack,
            db,
            locale_root: locale_root.clone(),
            openapi,
            api_url,
            res: spawn_handler(res_path),
            rev: RevService::new(tydb, locale_root, rev),
            sqlite_path,
            db_table_rels,
        }
    }

    fn db_api<T: Serialize>(
        &self,
        accept: Accept,
        f: impl FnOnce(Database<'static>) -> Result<T, CastError>,
    ) -> Result<Response<hyper::Body>, ApiError> {
        reply(accept, &f(self.db)?, StatusCode::OK)
    }

    fn db_api_opt<T: Serialize>(
        &self,
        accept: Accept,
        f: impl FnOnce(Database<'static>) -> Result<Option<T>, CastError>,
    ) -> Result<Response<hyper::Body>, ApiError> {
        reply_opt(accept, f(self.db)?.as_ref())
    }

    fn query_api(
        &self,
        f: impl FnOnce(&Path) -> Result<String, rusqlite::Error>,
    ) -> Result<Response<hyper::Body>, ApiError> {
        Ok(reply_string(f(self.sqlite_path)?, TEXT_CSV, StatusCode::OK))
    }

    fn graphql_api(
        &self,
        f: impl FnOnce(&Path, &graphql::TableRels) -> Result<String, graphql::QueryError>,
    ) -> Result<Response<hyper::Body>, ApiError> {
        Ok(reply_string(
            f(self.sqlite_path, self.db_table_rels)?,
            APPLICATION_JSON,
            StatusCode::OK,
        ))
    }

    /// Get data from `locale.xml`
    fn locale(&self, accept: Accept, rest: RestPath) -> Result<Response<hyper::Body>, ApiError> {
        match locale::select_node(self.locale_root.root.node(), rest) {
            Some((node, locale::Mode::All)) => {
                reply(accept, &locale::All::new(node), StatusCode::OK)
            }
            Some((node, locale::Mode::Pod)) => {
                reply(accept, &locale::Pod::new(node), StatusCode::OK)
            }
            None => Ok(reply_404()),
        }
    }

    fn swagger_ui_redirect(&self) -> Result<http::Response<hyper::Body>, ApiError> {
        let mut r = http::Response::new(hyper::Body::empty());
        *r.status_mut() = StatusCode::PERMANENT_REDIRECT;
        r.headers_mut().append(LOCATION, self.api_url.clone());
        Ok(r)
    }

    fn res_request(&self, accept: Accept, rest: RestPath) -> ApiFuture {
        ApiFuture::boxed({
            let sender = self.res.clone();
            let mut bytes = Vec::new();
            for part in rest.0 {
                bytes.push(b'\\');
                bytes.extend_from_slice(part.as_bytes());
            }
            async move {
                match sender.request(Bytes::from(bytes)).await {
                    Ok(v) => reply(accept, &v, StatusCode::OK),
                    Err(()) => Ok(reply_404()),
                }
            }
        })
    }
}

const SWAGGER_UI_HTML: &str = include_str!("../../res/api.html");

#[pin_project(project = ApiFutureProj)]
pub enum ApiFuture {
    Ready(#[pin] Ready<ApiResult>),
    Boxed(#[pin] BoxFuture<'static, ApiResult>),
}

impl ApiFuture {
    pub fn ready(value: ApiResult) -> Self {
        Self::Ready(ready(value))
    }

    pub fn boxed(f: impl Future<Output = ApiResult> + Send + 'static) -> Self {
        Self::Boxed(f.boxed())
    }
}

impl std::future::Future for ApiFuture {
    type Output = ApiResult;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ApiFutureProj::Ready(f) => f.poll(cx),
            ApiFutureProj::Boxed(f) => f.poll(cx),
        }
    }
}

static ALLOW_GET_HEAD: HeaderValue = HeaderValue::from_static("GET,HEAD");
static ALLOW_GET_HEAD_QUERY: HeaderValue = HeaderValue::from_static("GET,HEAD,QUERY");

#[derive(Deserialize)]
struct GraphQlRequest {
    query: String,
}

impl<ReqBody> Service<Request<ReqBody>> for ApiService
where
    ReqBody: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
    ReqBody::Error: fmt::Display,
{
    type Error = ApiError;
    type Response = ApiResponse;
    type Future = ApiFuture;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// This is the main entry point to the API service.
    ///
    /// Here, we turn [ApiRoute]s into [http::Response]s
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let accept = match parts.headers.get(ACCEPT) {
            Some(s) if s == "application/yaml" => Accept::Yaml,
            _ => Accept::Json,
        };
        let route = match ApiRoute::from_str(parts.uri.path()) {
            Ok(route) => {
                tracing::info!("API Route: {:?}", route);
                route
            }
            Err(()) => return ApiFuture::ready(Ok(reply_404())),
        };
        let method = parts.method;
        let response = match (method, route) {
            (Method::GET, ApiRoute::Tables) => self.db_api(accept, tables::tables_json),
            (Method::GET, ApiRoute::TableByName(name)) => {
                self.db_api(accept, |db| tables::table_def_json(db, name))
            }
            (method, ApiRoute::AllTableRows(name)) => match method.as_str() {
                "GET" => self.db_api_opt(accept, |db| tables::table_all_get(db, name)),
                "QUERY" => {
                    let name = name.to_owned();
                    let db = self.db;
                    return ApiFuture::boxed(async move {
                        tables::table_all_query(db, accept, &name, body).await
                    });
                }
                _ => Ok(reply_405(&ALLOW_GET_HEAD_QUERY)),
            },
            (Method::GET, ApiRoute::TableRowsByPK(name, key)) => {
                self.db_api_opt(accept, |db| tables::table_key_json(db, name, key))
            }
            (Method::GET, ApiRoute::Query(query)) => {
                self.query_api(|sqlite_path| query::query(sqlite_path, query))
            }
            (Method::GET, ApiRoute::GraphQl(query)) => {
                self.graphql_api(|sqlite_path, table_rels| {
                    graphql::graphql(sqlite_path, table_rels, query.borrow())
                })
            }
            (Method::POST, ApiRoute::GraphQl(_)) => {
                let sqlite_path = self.sqlite_path;
                let db_table_rels = self.db_table_rels;
                return ApiFuture::boxed(async move {
                    let bytes = match hyper::body::to_bytes(body).await {
                        Ok(x) => x,
                        Err(_) => {
                            return reply_400(accept, "malformed POST body", "could not read body");
                        }
                    };
                    let query = match std::str::from_utf8(&bytes) {
                        Ok(x) => x,
                        Err(_) => {
                            return reply_400(
                                accept,
                                "malformed POST body",
                                "could not parse body as utf8 string",
                            );
                        }
                    };
                    let query: GraphQlRequest = serde_json::from_str(query)?;
                    Ok(reply_string(
                        graphql::graphql(sqlite_path, db_table_rels, &query.query)?,
                        APPLICATION_JSON,
                        StatusCode::OK,
                    ))
                });
            }
            (method, ApiRoute::Locale(rest)) => match method {
                Method::GET => self.locale(accept, rest),
                m if m.as_str() == "QUERY" => {
                    return locale::locale_query(&self.locale_root, accept, rest, body)
                }
                _ => Ok(reply_405(&ALLOW_GET_HEAD_QUERY)),
            },
            (Method::GET, ApiRoute::OpenApiV0) => reply_json(self.openapi.as_ref(), StatusCode::OK),
            (Method::GET, ApiRoute::SwaggerUI) => Ok(reply_static(SWAGGER_UI_HTML)),
            (Method::GET, ApiRoute::SwaggerUIRedirect) => self.swagger_ui_redirect(),
            (Method::GET, ApiRoute::Crc(crc)) => {
                reply(accept, &self.pack.lookup(crc), StatusCode::OK)
            }
            (method, ApiRoute::Rev(route)) => {
                return ApiFuture::Ready(self.rev.call((accept, method, route)))
            }
            (Method::GET, ApiRoute::Res(rest)) => return self.res_request(accept, rest),
            (_, _) => Ok(reply_405(&ALLOW_GET_HEAD)),
        };
        ApiFuture::ready(response)
    }
}

/// Make the API
#[allow(clippy::too_many_arguments)]
pub fn service(
    cfg: &DataOptions,
    locale_root: LocaleRoot,
    auth_kind: AuthKind,
    base_url: String,
    db: Database<'static>,
    tydb: &'static TypedDatabase<'static>,
    rev: &'static ReverseLookup,
    db_table_rels: &'static graphql::TableRels,
    sqlite_path: &'static Path,
) -> Result<ApiService, color_eyre::Report> {
    // The pack service
    let res_path = cfg
        .res
        .as_deref()
        .unwrap_or_else(|| Path::new("client/res"));
    let pki_path = cfg.versions.as_ref().map(|x| x.join("primary.pki"));
    let pack = files::PackService::new(res_path, pki_path.as_deref())?;

    let api_url = base_url + router::API_PREFIX + "/";
    let openapi = docs::OpenApiService::new(&api_url, auth_kind)?;

    let api_uri = Uri::from_str(&api_url)?;
    Ok(ApiService::new(
        db,
        locale_root,
        pack,
        openapi,
        api_uri,
        tydb,
        rev,
        db_table_rels,
        res_path,
        sqlite_path,
    ))
}
