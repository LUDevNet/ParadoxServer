use std::collections::{BTreeMap, BTreeSet};

use paradox_typed_db::TypedDatabase;
use serde::Serialize;

use super::ReverseLookup;

#[derive(Serialize)]
struct LootMatrixResult {}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LootMatrixIndexRev {
    #[serde(skip_serializing_if = "LootMatrixIndexComponents::is_empty")]
    pub(crate) components: LootMatrixIndexComponents,
    /// Map from `ActivityRewards::ActivityRewardIndex` to `ActivityRewards::objectTemplate`
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) activity_rewards: BTreeMap<i32, i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LootMatrixIndexComponents {
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) smashable: BTreeSet<i32>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) package: BTreeSet<i32>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) destructible: BTreeSet<i32>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) vendor: BTreeSet<i32>,
}

impl LootMatrixIndexComponents {
    fn is_empty(&self) -> bool {
        self.smashable.is_empty()
            && self.package.is_empty()
            && self.destructible.is_empty()
            && self.vendor.is_empty()
    }
}

pub(super) fn rev_loop_table_index<'r>(
    _db: &'r TypedDatabase<'_>,
    rev: &'r ReverseLookup,
    index: i32,
) -> Option<&'r LootMatrixIndexRev> {
    rev.loot_matrix_index.get(&index)
}
