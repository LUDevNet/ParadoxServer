use std::collections::{BTreeMap, BTreeSet};

use paradox_typed_db::TypedDatabase;
use serde::Serialize;

use super::ReverseLookup;

#[derive(Serialize)]
struct LootMatrixResult {}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LootMatrixIndexRev {
    pub(crate) components: LootMatrixIndexComponents,
    /// Map from `ActivityRewards::ActivityRewardIndex` to `ActivityRewards::objectTemplate`
    pub(crate) activity_rewards: BTreeMap<i32, i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LootMatrixIndexComponents {
    pub(crate) smashable: BTreeSet<i32>,
    pub(crate) package: BTreeSet<i32>,
    pub(crate) destructible: BTreeSet<i32>,
    pub(crate) vendor: BTreeSet<i32>,
}

pub(super) fn rev_loop_table_index<'db, 'r>(
    db: &'r TypedDatabase<'db>,
    rev: &'r ReverseLookup,
    index: i32,
) -> Option<&'r LootMatrixIndexRev> {
    rev.loot_matrix_index.get(&index)
}
