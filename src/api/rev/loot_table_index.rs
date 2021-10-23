use std::convert::Infallible;

use crate::api::{
    adapter::{AdapterLayout, TypedTableIterAdapter},
    map_opt,
};
use paradox_typed_db::{typed_rows::LootTableRow, TypedDatabase};
use serde::Serialize;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use super::{Ext, Rev};

#[derive(Clone, Serialize)]
pub struct LootTableResult<T> {
    loot_table: T,
}

fn rev_loop_table_index_api(db: &TypedDatabase, rev: Rev<'static>, index: i32) -> Option<Json> {
    rev.inner
        .loot_table_index
        .get(&index)
        .map(|g| {
            let keys = g.items.keys().copied();
            let index = &g.items;
            let list: TypedTableIterAdapter<LootTableRow, _, _> = TypedTableIterAdapter {
                index,
                keys,
                table: &db.loot_table,
                id_col: db.loot_table.col_id,
                layout: AdapterLayout::Seq,
            };
            list
        })
        .map(|loot_table| warp::reply::json(&LootTableResult { loot_table }))
}

pub(super) fn loot_table_index_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_loot_table_index_base = rev.clone().and(warp::path("loot_table_index"));

    rev_loot_table_index_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_loop_table_index_api)
        .map(map_opt)
        .boxed()
}
