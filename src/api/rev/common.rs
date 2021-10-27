use std::collections::HashMap;

use assembly_fdb::common::Latin1Str;
use paradox_typed_db::{
    columns::ObjectsColumn,
    rows::{MissionTasksRow, ObjectsRow},
    tables::{MissionTasksTable, ObjectsTable},
};
use serde::Serialize;

use crate::api::adapter::{
    FindHash, I32Slice, IdentityHash, TableMultiIter, TypedTableIterAdapter,
};

use super::data::MissionTaskUIDLookup;

#[derive(Debug, Clone)]
pub struct MapFilter<'a, E> {
    base: &'a HashMap<i32, E>,
    keys: &'a [i32],
}

#[derive(Clone)]
pub struct ObjectsRefAdapter<'a, 'b> {
    table: &'b ObjectsTable<'a>,
    keys: &'b [i32],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
struct ObjectRefData<'a> {
    name: &'a Latin1Str,
}

impl<'a, 'b> ObjectsRefAdapter<'a, 'b> {
    pub fn new(table: &'b ObjectsTable<'a>, keys: &'b [i32]) -> Self {
        Self { table, keys }
    }
}

impl<'a, 'b> serde::Serialize for ObjectsRefAdapter<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let id_col = self.table.get_col(ObjectsColumn::Id).unwrap();
        serializer.collect_map(
            TableMultiIter {
                index: IdentityHash,
                key_iter: self.keys.iter().copied(),
                table: self.table,
                id_col,
            }
            .map(|(id, row): (i32, ObjectsRow)| (id, ObjectRefData { name: row.name() })),
        )
    }
}

#[derive(Serialize)]
pub(super) struct ObjectTypeEmbedded<'a, 'b> {
    pub objects: ObjectsRefAdapter<'a, 'b>,
}

impl<'a, E> MapFilter<'a, E> {
    fn to_iter<'b: 'a>(&'b self) -> impl Iterator<Item = (i32, &'a E)> + 'b {
        self.keys
            .iter()
            .filter_map(move |k| self.base.get(k).map(move |v| (*k, v)))
    }
}

impl<'a, E: Serialize> Serialize for MapFilter<'a, E> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.to_iter())
    }
}

pub(super) type MissionTaskHash<'b> = &'b HashMap<i32, MissionTaskUIDLookup>;

pub(super) type MissionTasks<'a, 'b> =
    TypedTableIterAdapter<'a, 'b, MissionTasksRow<'a, 'b>, MissionTaskHash<'b>, I32Slice<'b>>;

pub(super) struct MissionTaskIconsAdapter<'a, 'b> {
    table: &'b MissionTasksTable<'a>,
    key: i32,
}

impl<'a, 'b> Serialize for MissionTaskIconsAdapter<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.table.as_task_icon_iter(self.key))
    }
}

#[derive(Clone)]
pub(super) struct MissionsTaskIconsAdapter<'a, 'b> {
    table: &'b MissionTasksTable<'a>,
    keys: I32Slice<'b>,
}

impl<'a, 'b> MissionsTaskIconsAdapter<'a, 'b> {
    pub fn new(table: &'b MissionTasksTable<'a>, keys: &'b [i32]) -> Self {
        Self {
            table,
            keys: I32Slice(keys),
        }
    }
}

impl<'a, 'b> Serialize for MissionsTaskIconsAdapter<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.keys.into_iter().map(|key| {
            (
                key,
                MissionTaskIconsAdapter {
                    table: self.table,
                    key,
                },
            )
        }))
    }
}

impl<'a> FindHash for HashMap<i32, MissionTaskUIDLookup> {
    fn find_hash(&self, v: i32) -> Option<i32> {
        self.get(&v).map(|r| r.mission)
    }
}
