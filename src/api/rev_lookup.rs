use assembly_core::buffer::CastError;
use assembly_data::{fdb::common::Latin1Str, xml::localization::LocaleNode};
use paradox_typed_db::{
    typed_rows::{BehaviorTemplateRow, MissionTaskRow, MissionsRow, TypedRow},
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
    reply::{Json, WithStatus},
    Filter, Rejection,
};

use crate::{
    api::adapter::{FindHash, IdentityHash, TypedTableIterAdapter},
    data::skill_system::match_action_key,
};

use super::{adapter::LocaleTableAdapter, map_res, tydb_filter, PercentDecoded};

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

#[derive(Default, Debug, Clone, Serialize)]
pub struct SkillIdLookup {
    /// This field collects all the `uid`s of mission tasks that use this skill
    ///
    pub mission_tasks: Vec<i32>,
    /// The objects that can cast this skill
    pub objects: Vec<i32>,
    /// The item sets that enable this skill
    pub item_sets: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionTaskUIDLookup {
    pub mission: i32,
}

impl<'a> FindHash for HashMap<i32, MissionTaskUIDLookup> {
    fn find_hash(&self, v: i32) -> Option<i32> {
        self.get(&v).map(|r| r.mission)
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BehaviorKeyIndex {
    skill: BTreeSet<i32>,
    uses: BTreeSet<i32>,
    used_by: BTreeSet<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReverseLookup {
    pub mission_task_uids: HashMap<i32, MissionTaskUIDLookup>,
    pub skill_ids: HashMap<i32, SkillIdLookup>,
    pub behaviors: BTreeMap<i32, BehaviorKeyIndex>,
    pub mission_types: BTreeMap<String, BTreeMap<String, Vec<i32>>>,
}

impl ReverseLookup {
    pub(crate) fn new(db: &'_ TypedDatabase<'_>) -> Self {
        let mut skill_ids: HashMap<i32, SkillIdLookup> = HashMap::new();
        let mut mission_task_uids = HashMap::new();
        let mut mission_types: BTreeMap<String, BTreeMap<String, Vec<i32>>> = BTreeMap::new();

        for m in db.missions.row_iter() {
            let id = m.id();
            let d_type = m
                .defined_type()
                .map(Latin1Str::decode)
                .unwrap_or_default()
                .into_owned();
            let d_subtype = m
                .defined_subtype()
                .map(Latin1Str::decode)
                .unwrap_or_default()
                .into_owned();
            mission_types
                .entry(d_type)
                .or_default()
                .entry(d_subtype)
                .or_default()
                .push(id)
        }

        for r in db.mission_tasks.row_iter() {
            let uid = r.uid();
            let id = r.id();
            mission_task_uids.insert(uid, MissionTaskUIDLookup { mission: id });

            if r.task_type() == 10 {
                if let Some(p) = r.task_param1() {
                    for num in p.decode().split(',').map(str::parse).filter_map(Result::ok) {
                        skill_ids.entry(num).or_default().mission_tasks.push(uid);
                    }
                }
            }
            //skill_ids.entry(r.uid()).or_default().mission_tasks.push(r
        }
        for s in db.object_skills.row_iter() {
            skill_ids
                .entry(s.skill_id())
                .or_default()
                .objects
                .push(s.object_template());
        }
        for s in db.item_set_skills.row_iter() {
            skill_ids
                .entry(s.skill_id())
                .or_default()
                .item_sets
                .push(s.skill_set_id());
        }

        let mut behaviors: BTreeMap<i32, BehaviorKeyIndex> = BTreeMap::new();
        for bp in db.behavior_parameters.row_iter() {
            let parameter_id = bp.parameter_id();
            let behavior_id = bp.behavior_id();
            if match_action_key(parameter_id) {
                let value = bp.value() as i32;
                behaviors.entry(behavior_id).or_default().uses.insert(value);
                behaviors
                    .entry(value)
                    .or_default()
                    .used_by
                    .insert(behavior_id);
            }
        }

        for skill in db.skills.row_iter() {
            let bid = skill.behavior_id();
            let skid = skill.skill_id();
            behaviors.entry(bid).or_default().skill.insert(skid);
        }

        Self {
            behaviors,
            skill_ids,
            mission_task_uids,
            mission_types,
        }
    }

    pub(crate) fn get_behavior_set(&self, root: i32) -> BTreeSet<i32> {
        let mut todo = Vec::new();
        let mut all = BTreeSet::new();
        todo.push(root);

        while let Some(next) = todo.pop() {
            if !all.contains(&next) {
                all.insert(next);
                if let Some(data) = self.behaviors.get(&next) {
                    todo.extend(data.uses.iter().filter(|&&x| x > 0));
                }
            }
        }
        all
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

pub(super) fn make_api_rev<'r>(
    db: &'r TypedDatabase<'r>,
    rev: &'r ReverseLookup,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Send + 'r {
    let db = tydb_filter(db);
    let rev = db.and(rev_filter(rev));

    let rev_skill_ids = rev.clone().and(warp::path("skill_ids"));
    let rev_skill_id_base = rev_skill_ids.and(warp::path::param::<i32>());
    let rev_skill_id = rev_skill_id_base
        .and(warp::path::end())
        .map(rev_skill_id_api)
        .map(map_res);

    let rev_mission_types_base = rev.clone().and(warp::path("mission_types"));

    let rev_mission_types_full = rev_mission_types_base
        .clone()
        .and(warp::path("full"))
        .and(warp::path::end())
        .map(rev_mission_types_full_api)
        .map(map_res);

    let rev_mission_type = rev_mission_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_mission_type_api)
        .map(map_res);

    let rev_mission_subtype = rev_mission_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_mission_subtype_api)
        .map(map_res);

    let rev_mission_types_list = rev_mission_types_base
        .clone()
        .and(warp::path::end())
        .map(rev_mission_types_api)
        .map(map_res);

    let rev_mission_types = rev_mission_types_full
        .or(rev_mission_type)
        .unify()
        .or(rev_mission_subtype)
        .unify()
        .or(rev_mission_types_list)
        .unify();

    let rev_behaviors = rev.clone().and(warp::path("behaviors"));
    let rev_behavior_id_base = rev_behaviors.and(warp::path::param::<i32>());
    let rev_behavior_id = rev_behavior_id_base
        .and(warp::path::end())
        .map(rev_behavior_api)
        .map(map_res);

    let first = rev.clone().and(warp::path::end()).map(rev_api).map(map_res);
    first
        .or(rev_skill_id)
        .unify()
        .or(rev_mission_types)
        .unify()
        .or(rev_behavior_id)
        .unify()
}
