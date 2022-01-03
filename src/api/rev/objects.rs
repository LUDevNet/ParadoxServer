use std::convert::Infallible;

use paradox_typed_db::TypedDatabase;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use crate::api::wrap_200;

use super::{Ext, Rev};

fn rev_objects_search_index_api(_db: &TypedDatabase, rev: Rev) -> Json {
    warp::reply::json(&rev.inner.objects.search_index)
}

pub(super) fn objects_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_objects_base = rev.clone().and(warp::path("objects"));

    rev_objects_base
        .clone()
        .and(warp::path("search_index"))
        .and(warp::path::end())
        .map(rev_objects_search_index_api)
        .map(wrap_200)
        .boxed()
}
