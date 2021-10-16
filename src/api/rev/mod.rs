//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
use assembly_core::buffer::CastError;
use assembly_data::xml::localization::LocaleNode;
use paradox_typed_db::{
    typed_rows::{BehaviorTemplateRow, MissionTaskRow, MissionsRow, ObjectsRef, TypedRow},
    typed_tables::{BehaviorParameterTable, BehaviorTemplateTable, MissionTasksTable},
    TypedDatabase,
};
use serde::{ser::SerializeMap, Serialize};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::Infallible,
};
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

mod data;

pub use data::ReverseLookup;

use self::data::MissionTaskUIDLookup;
use super::{adapter::LocaleTableAdapter, map_opt_res, map_res, tydb_filter, PercentDecoded};
use crate::api::adapter::{FindHash, IdentityHash, TypedTableIterAdapter};

#[derive(Debug, Clone, Serialize)]
pub struct Api<T, E> {
    #[serde(flatten)]
    data: T,
    #[serde(rename = "_embedded")]
    embedded: E,
}

#[derive(Debug, Clone)]
pub struct MapFilter<'a, E> {
    base: &'a HashMap<i32, E>,
    keys: &'a [i32],
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

type MissionTaskHash<'b> = &'b HashMap<i32, MissionTaskUIDLookup>;

type MissionTasks<'a, 'b> =
    TypedTableIterAdapter<'a, 'b, MissionTaskRow<'a, 'b>, MissionTaskHash<'b>, &'b [i32]>;

type MissionsAdapter<'a, 'b> =
    TypedTableIterAdapter<'a, 'b, MissionsRow<'a, 'b>, IdentityHash, &'b [i32]>;

struct MissionTaskIconsAdapter<'a, 'b> {
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
struct MissionsTaskIconsAdapter<'a, 'b> {
    table: &'b MissionTasksTable<'a>,
    keys: &'b [i32],
}

impl<'a, 'b> MissionsTaskIconsAdapter<'a, 'b> {
    pub fn new(table: &'b MissionTasksTable<'a>, keys: &'b [i32]) -> Self {
        Self { table, keys }
    }
}

impl<'a, 'b> Serialize for MissionsTaskIconsAdapter<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.keys.iter().copied().map(|key| {
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

#[derive(Clone, Serialize)]
pub struct SkillIDEmbedded<'a, 'b> {
    #[serde(rename = "MissionTasks")]
    mission_tasks: MissionTasks<'a, 'b>,
    //MapFilter<'a, MissionTaskUIDLookup>,
}

impl<'a> FindHash for HashMap<i32, MissionTaskUIDLookup> {
    fn find_hash(&self, v: i32) -> Option<i32> {
        self.get(&v).map(|r| r.mission)
    }
}

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

fn rev_api(_db: &TypedDatabase, _rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&["skill_ids"]))
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

#[derive(Debug, Clone)]
struct MissionSubtypes<'a>(&'a BTreeMap<String, Vec<i32>>);
#[derive(Debug, Clone)]
struct MissionTypes<'a>(&'a BTreeMap<String, BTreeMap<String, Vec<i32>>>);

impl<'a> Serialize for MissionSubtypes<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.0.keys())
    }
}

impl<'a> Serialize for MissionTypes<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            m.serialize_entry(key, &MissionSubtypes(value))?;
        }
        m.end()
    }
}

fn rev_mission_types_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&MissionTypes(&rev.inner.mission_types)))
}

#[derive(Clone, Serialize)]
struct MissionLocale<'b> {
    #[serde(rename = "MissionText")]
    mission_text: LocaleTableAdapter<'b>,
    #[serde(rename = "Missions")]
    missions: LocaleTableAdapter<'b>,
}

impl<'b> MissionLocale<'b> {
    pub fn new(node: &'b LocaleNode, keys: &'b [i32]) -> Self {
        Self {
            mission_text: LocaleTableAdapter::new(
                node.str_children.get("MissionText").unwrap(),
                keys,
            ),
            missions: LocaleTableAdapter::new(node.str_children.get("Missions").unwrap(), keys),
        }
    }
}

/// This is the root type that holds all embedded value for the `mission_types` lookup
#[derive(Clone, Serialize)]
struct MissionTypesEmbedded<'a, 'b> {
    #[serde(rename = "Missions")]
    missions: MissionsAdapter<'a, 'b>,
    #[serde(rename = "MissionTaskIcons")]
    mission_task_icons: MissionsTaskIconsAdapter<'a, 'b>,
    locale: MissionLocale<'b>,
}

#[derive(Debug, Clone, Serialize)]
struct Subtypes<'a> {
    subtypes: MissionSubtypes<'a>,
}

#[derive(Debug, Clone, Serialize)]
struct MissionIDList<'b> {
    mission_ids: &'b [i32],
}

fn missions_reply<'a, 'b>(
    db: &'b TypedDatabase<'a>,
    mission_ids: &'b [i32],
) -> Api<MissionIDList<'b>, MissionTypesEmbedded<'a, 'b>> {
    Api {
        data: MissionIDList { mission_ids },
        embedded: MissionTypesEmbedded {
            missions: MissionsAdapter::new(&db.missions, mission_ids),
            mission_task_icons: MissionsTaskIconsAdapter::new(&db.mission_tasks, mission_ids),
            locale: MissionLocale::new(&db.locale, mission_ids),
        },
    }
}

fn rev_mission_type_api(
    db: &TypedDatabase,
    rev: Rev,
    d_type: PercentDecoded,
) -> Result<Json, CastError> {
    let key: &String = d_type.borrow();
    match rev.inner.mission_types.get(key) {
        Some(t) => match t.get("") {
            Some(missions) => Ok(warp::reply::json(&missions_reply(db, missions))),
            None => Ok(warp::reply::json(&Subtypes {
                subtypes: MissionSubtypes(t),
            })),
        },
        None => Ok(warp::reply::json(&())),
    }
}

fn rev_mission_subtype_api(
    db: &TypedDatabase,
    rev: Rev,
    d_type: PercentDecoded,
    d_subtype: PercentDecoded,
) -> Result<Json, CastError> {
    let t_key: &String = d_type.borrow();
    let t = rev.inner.mission_types.get(t_key);
    let s_key: &String = d_subtype.borrow();
    let s = t.and_then(|t| t.get(s_key));
    let m = s.map(|missions| missions_reply(db, missions));
    Ok(warp::reply::json(&m))
}

fn rev_mission_types_full_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&rev.inner.mission_types))
}

#[derive(Serialize)]
struct ObjectIDs<'a, T> {
    object_ids: &'a [T],
}

type ObjectsRefAdapter<'a, 'b> =
    TypedTableIterAdapter<'a, 'b, ObjectsRef<'a, 'b>, IdentityHash, &'b [i32]>;

#[derive(Serialize)]
struct ObjectTypeEmbedded<'a, 'b> {
    objects: ObjectsRefAdapter<'a, 'b>,
}

fn rev_object_type_api(
    db: &TypedDatabase,
    rev: Rev,
    ty: PercentDecoded,
) -> Result<Option<Json>, CastError> {
    let key: &String = ty.borrow();
    tracing::info!("{}", key);
    Ok(rev.inner.object_types.get(key).map(|objects| {
        let rep = Api {
            data: ObjectIDs {
                object_ids: objects.as_ref(),
            },
            embedded: ObjectTypeEmbedded {
                objects: TypedTableIterAdapter::new(&db.objects, objects),
            },
        };
        warp::reply::json(&rep)
    }))
}

fn rev_object_types_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    let keys: Vec<_> = rev.inner.object_types.keys().collect();
    Ok(warp::reply::json(&keys))
}

#[derive(Serialize)]
struct Components {
    components: Vec<i32>,
}

fn rev_component_types_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    let components: Vec<i32> = rev.inner.component_use.keys().copied().collect();
    let val = Components { components };
    Ok(warp::reply::json(&val))
}

fn rev_component_type_api(
    _db: &TypedDatabase,
    rev: Rev,
    key: i32,
) -> Result<Option<Json>, CastError> {
    let val = rev.inner.component_use.get(&key);
    Ok(val.map(|data| {
        let keys: Vec<i32> = data
            .components
            .iter()
            .flat_map(|(_, u)| u.lots.iter().copied())
            .collect();
        let embedded = ObjectTypeEmbedded {
            objects: ObjectsRefAdapter::new(&_db.objects, &keys),
        };
        warp::reply::json(&Api { data, embedded })
    }))
}

fn rev_single_component_api(
    _db: &TypedDatabase,
    rev: Rev,
    key: i32,
    cid: i32,
) -> Result<Option<Json>, CastError> {
    let val = rev
        .inner
        .component_use
        .get(&key)
        .and_then(|c| c.components.get(&cid));
    Ok(val.map(warp::reply::json))
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

fn rev_skill_id_api(db: &'_ TypedDatabase, rev: Rev, skill_id: i32) -> Result<Json, CastError> {
    let h = rev.inner.skill_ids.get(&skill_id).map(|data| {
        let mission_tasks = MissionTasks {
            index: &rev.inner.mission_task_uids,
            keys: &data.mission_tasks[..],
            table: &db.mission_tasks,
            id_col: db.mission_tasks.col_uid,
        };
        Api {
            data,
            embedded: SkillIDEmbedded { mission_tasks },
        }
    });
    Ok(warp::reply::json(&h))
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Rev<'a> {
    inner: &'a ReverseLookup,
}

fn rev_filter<'a>(
    inner: &'a ReverseLookup,
) -> impl Filter<Extract = (Rev,), Error = Infallible> + Clone + 'a {
    warp::any().map(move || Rev { inner })
}

type Ext = (&'static TypedDatabase<'static>, Rev<'static>);

fn skill_api<F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_skill_ids = rev.clone().and(warp::path("skill_ids"));
    let rev_skill_id_base = rev_skill_ids.and(warp::path::param::<i32>());
    let rev_skill_id = rev_skill_id_base
        .and(warp::path::end())
        .map(rev_skill_id_api)
        .map(map_res);
    rev_skill_id.boxed()
}

fn mission_types_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_mission_types_base = rev.clone().and(warp::path("mission_types"));

    let rev_mission_types_full = rev_mission_types_base
        .clone()
        .and(warp::path("full"))
        .and(warp::path::end())
        .map(rev_mission_types_full_api)
        .map(map_res)
        .boxed();

    let rev_mission_type = rev_mission_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_mission_type_api)
        .map(map_res)
        .boxed();

    let rev_mission_subtype = rev_mission_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_mission_subtype_api)
        .map(map_res)
        .boxed();

    let rev_mission_types_list = rev_mission_types_base
        .clone()
        .and(warp::path::end())
        .map(rev_mission_types_api)
        .map(map_res)
        .boxed();

    let rev_mission_types = rev_mission_types_full
        .or(rev_mission_type)
        .unify()
        .or(rev_mission_subtype)
        .unify()
        .or(rev_mission_types_list)
        .unify();

    rev_mission_types.boxed()
}

fn component_types_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_component_types_base = rev.clone().and(warp::path("component_types"));

    let rev_single_component_type = rev_component_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_single_component_api)
        .map(map_opt_res)
        .boxed();

    let rev_component_type = rev_component_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_component_type_api)
        .map(map_opt_res)
        .boxed();

    let rev_component_types_list = rev_component_types_base
        .clone()
        .and(warp::path::end())
        .map(rev_component_types_api)
        .map(map_res)
        .boxed();

    rev_single_component_type
        .or(rev_component_type)
        .unify()
        .or(rev_component_types_list)
        .unify()
        .boxed()
}

fn object_types_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_object_types_base = rev.clone().and(warp::path("object_types"));

    let rev_object_type = rev_object_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_object_type_api)
        .map(map_opt_res)
        .boxed();

    let rev_object_types_list = rev_object_types_base
        .clone()
        .and(warp::path::end())
        .map(rev_object_types_api)
        .map(map_res)
        .boxed();

    rev_object_type.or(rev_object_types_list).unify().boxed()
}

fn behaviors_api<F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static>(
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

pub(super) fn make_api_rev(
    db: &'static TypedDatabase<'static>,
    rev: &'static ReverseLookup,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let db = tydb_filter(db);
    let rev = db.and(rev_filter(rev));

    let rev_skills = skill_api(&rev);
    let rev_mission_types = mission_types_api(&rev);
    let rev_object_types = object_types_api(&rev);
    let rev_component_types = component_types_api(&rev);
    let rev_behaviors = behaviors_api(&rev);

    let first = rev
        .clone()
        .and(warp::path::end())
        .map(rev_api)
        .map(map_res)
        .boxed();
    first
        .or(rev_skills)
        .unify()
        .or(rev_mission_types)
        .unify()
        .or(rev_object_types)
        .unify()
        .or(rev_component_types)
        .unify()
        .or(rev_behaviors)
        .unify()
        .boxed()
}
