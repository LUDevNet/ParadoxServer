use assembly_core::buffer::CastError;
use paradox_typed_db::{
    typed_rows::{BehaviorTemplateRow, TypedRow},
    typed_tables::{BehaviorParameterTable, BehaviorTemplateTable},
    TypedDatabase,
};
use serde::ser::SerializeMap;
use serde::Serialize;

use crate::api::map_res;
use std::{collections::BTreeSet, convert::Infallible};

use super::{Api, Ext, Rev};
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

#[derive(Clone)]
pub(crate) struct BehaviorParameters<'a, 'b> {
    key: i32,
    table: &'b BehaviorParameterTable<'a>,
}

impl Serialize for BehaviorParameters<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(None)?;
        for e in self.table.key_iter(self.key) {
            m.serialize_key(e.parameter_id())?;
            m.serialize_value(&e.value())?;
        }
        m.end()
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Behavior<'a, 'b> {
    #[serde(flatten)]
    template: Option<BehaviorTemplateRow<'a, 'b>>,
    parameters: BehaviorParameters<'a, 'b>,
}

struct EmbeddedBehaviors<'a, 'b> {
    keys: &'b BTreeSet<i32>,
    table_templates: &'b BehaviorTemplateTable<'a>,
    table_parameters: &'b BehaviorParameterTable<'a>,
}

impl Serialize for EmbeddedBehaviors<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(Some(self.keys.len()))?;
        for &behavior_id in self.keys {
            m.serialize_key(&behavior_id)?;
            let b = Behavior {
                template: BehaviorTemplateRow::get(
                    self.table_templates,
                    behavior_id,
                    behavior_id,
                    self.table_templates.col_behavior_id,
                ),
                parameters: BehaviorParameters {
                    key: behavior_id,
                    table: self.table_parameters,
                },
            };
            m.serialize_value(&b)?;
        }
        m.end()
    }
}

fn rev_behavior_api(db: &TypedDatabase, rev: Rev, behavior_id: i32) -> Result<Json, CastError> {
    let data = rev.inner.behaviors.get(&behavior_id);
    let set = rev.inner.get_behavior_set(behavior_id);
    let val = Api {
        data,
        embedded: EmbeddedBehaviors {
            keys: &set,
            table_templates: &db.behavior_templates,
            table_parameters: &db.behavior_parameters,
        },
    };
    Ok(warp::reply::json(&val))
}

pub(super) fn behaviors_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_behaviors = rev.clone().and(warp::path("behaviors"));
    let rev_behavior_id_base = rev_behaviors.and(warp::path::param::<i32>());
    let rev_behavior_id = rev_behavior_id_base
        .and(warp::path::end())
        .map(rev_behavior_api)
        .map(map_res);

    rev_behavior_id.boxed()
}
