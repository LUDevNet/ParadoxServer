//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
use assembly_core::buffer::CastError;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::convert::Infallible;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

mod common;
mod data;

mod behaviors;
mod component_types;
mod missions;
mod object_types;
mod skills;

pub use data::ReverseLookup;

use super::{map_res, tydb_filter};

#[derive(Debug, Clone, Serialize)]
pub struct Api<T, E> {
    #[serde(flatten)]
    data: T,
    #[serde(rename = "_embedded")]
    embedded: E,
}

fn rev_api(_db: &TypedDatabase, _rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&[
        "behaviors",
        "component_types",
        "mission_types",
        "object_types",
        "skill_ids",
    ]))
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
    rev: &'static ReverseLookup,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let db = tydb_filter(db);
    let rev = db.and(rev_filter(rev));

    let rev_skills = skills::skill_api(&rev);
    let rev_mission_types = missions::mission_types_api(&rev);
    let rev_object_types = object_types::object_types_api(&rev);
    let rev_component_types = component_types::component_types_api(&rev);
    let rev_behaviors = behaviors::behaviors_api(&rev);

    let first = rev
        .clone()
        .and(warp::path::end())
        .map(rev_api)
        .map(map_res)
        .boxed();
    first
        .or(rev_skills)
        .unify()
        .or(rev_mission_types)
        .unify()
        .or(rev_object_types)
        .unify()
        .or(rev_component_types)
        .unify()
        .or(rev_behaviors)
        .unify()
        .boxed()
}
