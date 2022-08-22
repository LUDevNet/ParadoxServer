//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
use assembly_core::buffer::CastError;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::{
    convert::Infallible,
    io, str,
    task::{Context, Poll},
};
use tower::Service;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

mod common;
mod data;

mod activity;
mod behaviors;
mod component_types;
mod loot_table_index;
mod missions;
mod object_types;
mod objects;
mod skills;

pub use data::ReverseLookup;

use crate::data::locale::LocaleRoot;

use super::{map_res, tydb_filter};

#[derive(Debug, Clone, Serialize)]
pub struct Api<T, E> {
    #[serde(flatten)]
    data: T,
    #[serde(rename = "_embedded")]
    embedded: E,
}

static REV_APIS: &[&str; 8] = &[
    "activity",
    "behaviors",
    "component_types",
    "loot_table_index",
    "mission_types",
    "objects",
    "object_types",
    "skill_ids",
];

fn rev_api(_db: &TypedDatabase, _rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&REV_APIS))
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Rev<'a> {
    inner: &'a ReverseLookup,
}

fn rev_filter<'a>(
    inner: &'a ReverseLookup,
) -> impl Filter<Extract = (Rev,), Error = Infallible> + Clone + 'a {
    warp::any().map(move || Rev { inner })
}

type Ext = (&'static TypedDatabase<'static>, Rev<'static>);

pub(super) fn make_api_rev(
    db: &'static TypedDatabase<'static>,
    loc: LocaleRoot,
    rev: &'static ReverseLookup,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let db = tydb_filter(db);
    let rev = db.and(rev_filter(rev));

    let rev_activity = activity::activity_api(&rev);
    let rev_behaviors = behaviors::behaviors_api(&rev);
    let rev_component_types = component_types::component_types_api(&rev);
    let rev_loot_table_index = loot_table_index::loot_table_index_api(&rev);
    let rev_mission_types = missions::mission_types_api(&rev, loc);
    let rev_objects = objects::objects_api(&rev);
    let rev_object_types = object_types::object_types_api(&rev);
    let rev_skills = skills::skill_api(&rev);

    let first = rev
        .clone()
        .and(warp::path::end())
        .map(rev_api)
        .map(map_res)
        .boxed();
    first
        .or(rev_activity)
        .unify()
        .or(rev_skills)
        .unify()
        .or(rev_mission_types)
        .unify()
        .or(rev_object_types)
        .unify()
        .or(rev_objects)
        .unify()
        .or(rev_component_types)
        .unify()
        .or(rev_behaviors)
        .unify()
        .or(rev_loot_table_index)
        .unify()
        .boxed()
}

pub(super) enum Route {
    Base,
}

impl Route {
    pub(super) fn from_parts(mut parts: str::Split<'_, char>) -> Result<Self, ()> {
        match parts.next() {
            Some("") => match parts.next() {
                None => Ok(Self::Base),
                _ => Err(()),
            },
            None => Ok(Self::Base),
            _ => Err(()),
        }
    }
}

#[derive(Clone)]
pub struct RevService {
    db: &'static TypedDatabase<'static>,
    loc: LocaleRoot,
    rev: &'static ReverseLookup,
}

impl RevService {
    pub(crate) fn new(
        db: &'static TypedDatabase<'static>,
        loc: LocaleRoot,
        rev: &'static ReverseLookup,
    ) -> RevService {
        Self { db, loc, rev }
    }
}

impl Service<Route> for RevService {
    type Response = http::Response<hyper::Body>;
    type Error = io::Error;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Route) -> Self::Future {
        let r = match req {
            Route::Base => super::reply_json(&REV_APIS),
        };
        std::future::ready(r)
    }
}
