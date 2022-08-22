use paradox_typed_db::{
    columns::BehaviorTemplateColumn,
    rows::BehaviorTemplateRow,
    tables::{BehaviorParameterTable, BehaviorTemplateTable},
    TypedDatabase, TypedRow,
};
use serde::ser::SerializeMap;
use serde::Serialize;

use std::collections::BTreeSet;

use super::{data::BehaviorKeyIndex, Api, ReverseLookup};

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

pub(super) struct EmbeddedBehaviors<'a, 'b> {
    keys: BTreeSet<i32>,
    table_templates: &'b BehaviorTemplateTable<'a>,
    table_parameters: &'b BehaviorParameterTable<'a>,
}

impl Serialize for EmbeddedBehaviors<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(Some(self.keys.len()))?;
        let col_behavior_id = self
            .table_templates
            .get_col(BehaviorTemplateColumn::BehaviorId)
            .unwrap();
        for &behavior_id in &self.keys {
            m.serialize_key(&behavior_id)?;
            let b = Behavior {
                template: BehaviorTemplateRow::get(
                    self.table_templates,
                    behavior_id,
                    behavior_id,
                    col_behavior_id,
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

pub(super) fn lookup<'db, 'd, 'r>(
    db: &'d TypedDatabase<'db>,
    rev: &'r ReverseLookup,
    behavior_id: i32,
) -> Api<Option<&'r BehaviorKeyIndex>, EmbeddedBehaviors<'db, 'd>> {
    Api {
        data: rev.behaviors.get(&behavior_id),
        embedded: EmbeddedBehaviors {
            keys: rev.get_behavior_set(behavior_id),
            table_templates: &db.behavior_templates,
            table_parameters: &db.behavior_parameters,
        },
    }
}
