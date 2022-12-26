use std::collections::BTreeMap;

use paradox_typed_db::TypedDatabase;
use serde::Serialize;

use super::{
    common::{ObjectTypeEmbedded, ObjectsRefAdapter},
    data::{ComponentUse, ComponentsUse, ReverseLookup},
    Api,
};
use crate::api::adapter::Keys;

#[derive(Serialize)]
pub(super) struct Components<'a> {
    components: Keys<&'a BTreeMap<i32, ComponentsUse>>,
}

impl<'a> Components<'a> {
    pub fn new(rev: &'a ReverseLookup) -> Self {
        Self {
            components: Keys::new(&rev.component_use.0),
        }
    }
}

pub(super) fn rev_component_type<'r, 'db, 'd>(
    db: &'d TypedDatabase<'db>,
    rev: &'r ReverseLookup,
    key: i32,
) -> Option<Api<&'r ComponentsUse, ObjectTypeEmbedded<'db, 'd, Vec<i32>>>> {
    rev.component_use.ty(key).map(|data: &'r ComponentsUse| {
        // FIXME: improve this
        let keys: Vec<i32> = data
            .components
            .iter()
            .flat_map(|(_, u)| u.lots.iter().copied())
            .collect();
        let embedded = ObjectTypeEmbedded {
            objects: ObjectsRefAdapter::new(&db.objects, keys),
        };
        Api { data, embedded }
    })
}

pub(super) fn rev_single_component(
    rev: &ReverseLookup,
    key: i32,
    cid: i32,
) -> Option<&ComponentUse> {
    rev.component_use
        .ty(key)
        .and_then(|c| c.components.get(&cid))
}
