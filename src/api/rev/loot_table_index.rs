use std::{
    collections::{btree_map, BTreeMap},
    iter::Copied,
};

use crate::api::adapter::{AdapterLayout, TypedTableIterAdapter};
use paradox_typed_db::{columns::LootTableColumn, rows::LootTableRow, TypedDatabase};
use serde::Serialize;

use super::ReverseLookup;

type ResultInner<'db, 'r> = TypedTableIterAdapter<
    'db,
    'r,
    LootTableRow<'db, 'r>,
    &'r BTreeMap<i32, i32>,
    Copied<btree_map::Keys<'r, i32, i32>>,
>;

#[derive(Clone, Serialize)]
pub struct LootTableResult<'db, 'r> {
    loot_table: ResultInner<'db, 'r>,
}

pub(super) fn rev_loop_table_index<'db, 'r>(
    db: &'r TypedDatabase<'db>,
    rev: &'r ReverseLookup,
    index: i32,
) -> Option<LootTableResult<'db, 'r>> {
    let lti_rev = rev.loot_table_index.get(&index)?;
    let keys = lti_rev.items.keys().copied();
    let index = &lti_rev.items;
    let loot_table = TypedTableIterAdapter {
        index,
        keys,
        table: &db.loot_table,
        id_col: db.loot_table.get_col(LootTableColumn::Id).unwrap(),
        layout: AdapterLayout::Seq,
    };
    Some(LootTableResult { loot_table })
}
