use std::{
    borrow::Borrow,
    convert::Infallible,
    error::Error,
    str::{FromStr, Utf8Error},
    sync::Arc,
};

use assembly_data::{fdb::mem::Database, xml::localization::LocaleNode};
use paradox_typed_db::TypedDatabase;
use percent_encoding::percent_decode_str;
use warp::{
    filters::BoxedFilter,
    path::Tail,
    reply::{Json, WithStatus},
    Filter, Reply,
};

use crate::auth::AuthKind;

use self::{
    adapter::{LocaleAll, LocalePod},
    rev::{make_api_rev, ReverseLookup},
    tables::{make_api_tables, tables_api},
};

pub mod adapter;
mod docs;
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

/*fn copy_filter<'x, T>(v: T) -> impl Filter<Extract = (T,), Error=Infallible> + Clone + 'x where T: Send + Sync + Copy + 'x {
    warp::any().map(move || v)
}*/

fn db_filter<'db>(
    db: Database<'db>,
) -> impl Filter<Extract = (Database,), Error = Infallible> + Clone + 'db {
    warp::any().map(move || db)
}

fn tydb_filter<'db>(
    db: &'db TypedDatabase<'db>,
) -> impl Filter<Extract = (&'db TypedDatabase<'db>,), Error = Infallible> + Clone + 'db {
    warp::any().map(move || db)
}

pub fn locale_api(lr: Arc<LocaleNode>) -> impl Fn(Tail) -> Option<warp::reply::Json> + Clone {
    move |p: Tail| {
        let path = p.as_str().trim_end_matches('/');
        let mut node = lr.as_ref();
        let mut all = false;
        if !path.is_empty() {
            let path = match path.strip_suffix("/$all") {
                Some(prefix) => {
                    all = true;
                    prefix
                }
                None => path,
            };

            // Skip loop for root node
            for seg in path.split('/') {
                if let Some(new) = {
                    if let Ok(num) = seg.parse::<u32>() {
                        node.int_children.get(&num)
                    } else {
                        node.str_children.get(seg)
                    }
                } {
                    node = new;
                } else {
                    return None;
                }
            }
        }
        if all {
            Some(warp::reply::json(&LocaleAll::new(node)))
        } else {
            Some(warp::reply::json(&LocalePod {
                value: node.value.as_deref(),
                int_keys: node.int_children.keys().cloned().collect(),
                str_keys: node.str_children.keys().map(|s| s.as_ref()).collect(),
            }))
        }
    }
}

pub(crate) fn make_api(
    url: String,
    auth_kind: AuthKind,
    db: Database<'static>,
    tydb: &'static TypedDatabase<'static>,
    rev: &'static ReverseLookup,
    lr: Arc<LocaleNode>,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let v0_base = warp::path("v0");
    let v0_tables = warp::path("tables").and(make_api_tables(db));
    let v0_locale = warp::path("locale")
        .and(warp::path::tail())
        .map(locale_api(lr))
        .map(map_opt);

    let v0_rev = warp::path("rev").and(make_api_rev(tydb, rev));
    let v0_openapi = docs::openapi(url, auth_kind).unwrap();
    let v0 = v0_base.and(
        v0_tables
            .or(v0_locale)
            .unify()
            .or(v0_rev)
            .unify()
            .or(v0_openapi)
            .unify(),
    );

    // v1
    let dbf = db_filter(db);
    let v1_base = warp::path("v1");
    let v1_tables_base = dbf.and(warp::path("tables"));
    let v1_tables = v1_tables_base
        .and(warp::path::end())
        .map(tables_api)
        .map(map_res);
    let v1 = v1_base.and(v1_tables);

    // catch all
    let catch_all = make_api_catch_all();

    v0.or(v1).unify().or(catch_all).unify().boxed()
}
