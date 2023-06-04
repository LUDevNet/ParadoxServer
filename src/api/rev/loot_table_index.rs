use std::{
    collections::{btree_map, BTreeMap},
    iter::Copied,
};

use crate::api::adapter::{AdapterLayout, TypedTableIterAdapter};
use paradox_typed_db::{
    columns::{LootMatrixColumn, LootTableColumn},
    rows::{LootMatrixRow, LootTableRow},
    TypedDatabase,
};
use serde::Serialize;

use super::ReverseLookup;

type LootTableResultInner<'db, 'r> = TypedTableIterAdapter<
    'db,
    'r,
    LootTableRow<'db, 'r>,
    &'r BTreeMap<i32, i32>,
    Copied<btree_map::Keys<'r, i32, i32>>,
>;

type LootMatrixResultInner<'db, 'r> = TypedTableIterAdapter<
    'db,
    'r,
    LootMatrixRow<'db, 'r>,
    &'r BTreeMap<i32, i32>,
    Copied<btree_map::Keys<'r, i32, i32>>,
>;

#[derive(Clone, Serialize)]
pub struct LootTableResult<'db, 'r> {
    loot_table: LootTableResultInner<'db, 'r>,
    loot_matrix: LootMatrixResultInner<'db, 'r>,
}

pub(super) fn rev_loop_table_index<'db, 'r>(
    db: &'r TypedDatabase<'db>,
    rev: &'r ReverseLookup,
    index: i32,
) -> Option<LootTableResult<'db, 'r>> {
    let lti_rev = rev.loot_table_index.get(&index)?;
    let loot_table = TypedTableIterAdapter {
        index: &lti_rev.items,
        keys: lti_rev.items.keys().copied(),
        table: &db.loot_table,
        id_col: db.loot_table.get_col(LootTableColumn::Id).unwrap(),
        layout: AdapterLayout::Seq,
    };
    let loot_matrix = TypedTableIterAdapter {
        index: &lti_rev.loot_matrix,
        keys: lti_rev.loot_matrix.keys().copied(),
        table: &db.loot_matrix,
        id_col: db.loot_matrix.get_col(LootMatrixColumn::Id).unwrap(),
        layout: AdapterLayout::Seq,
    };
    Some(LootTableResult {
        loot_table,
        loot_matrix,
    })
}
