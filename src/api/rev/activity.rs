use std::convert::Infallible;

use crate::api::map_opt;
use paradox_typed_db::TypedDatabase;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use super::{Ext, Rev};

fn rev_activity_api(_db: &TypedDatabase, rev: Rev<'static>, index: i32) -> Option<Json> {
    rev.inner.activities.get(&index).map(warp::reply::json)
}

pub(super) fn activity_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_loot_table_index_base = rev.clone().and(warp::path("activity"));

    rev_loot_table_index_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_activity_api)
        .map(map_opt)
        .boxed()
}
