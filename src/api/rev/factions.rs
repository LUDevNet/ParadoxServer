use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::api::adapter::Filtered2;

use super::{
    data::{ComponentUse, COMPONENT_ID_DESTRUCTIBLE},
    ReverseLookup,
};

#[derive(Serialize)]
struct FactionByIdEmbedded {
    destructible_components: Filtered2<BTreeMap<i32, ComponentUse>, &'static BTreeSet<i32>>,
}

#[derive(Serialize)]
pub(super) struct FactionById {
    destructible_ids: &'static BTreeSet<i32>,
    destructible_list_ids: &'static BTreeSet<i32>,
    _embedded: FactionByIdEmbedded,
}

impl FactionById {
    pub fn new(rev: &'static ReverseLookup, id: i32) -> Option<Self> {
        let frev = rev.factions.get(&id)?;
        Some(Self {
            destructible_ids: &frev.destructible,
            destructible_list_ids: &frev.destructible_list,
            _embedded: FactionByIdEmbedded {
                destructible_components: Filtered2 {
                    inner: &rev
                        .component_use
                        .ty(COMPONENT_ID_DESTRUCTIBLE)
                        .unwrap()
                        .components,
                    keys1: &frev.destructible,
                    keys2: &frev.destructible_list,
                },
            },
        })
    }
}
