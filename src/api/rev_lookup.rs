use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::Infallible,
    marker::PhantomData,
};

use crate::{
    data::skill_system::match_action_key,
    typed_db::{
        typed_rows::{BehaviorTemplateRow, MissionTaskRow, TypedRow},
        BehaviorParameterTable, BehaviorTemplateTable, FindHash, MissionTasksTable, TypedDatabase,
        TypedTableIterAdapter,
    },
};
use assembly_core::buffer::CastError;
use serde::{ser::SerializeMap, Serialize};
use warp::{
    reply::{Json, WithStatus},
    Filter, Rejection,
};

use super::{map_res, tydb_filter};

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

type MissionTasks<'a, 'b> = TypedTableIterAdapter<
    'b,
    MissionTasksTable<'a>,
    MissionTaskRow<'a, 'b>,
    &'b HashMap<i32, MissionTaskUIDLookup>,
    &'b [i32],
>;

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
}

impl ReverseLookup {
    pub(crate) fn new(db: &'_ TypedDatabase<'_>) -> Self {
        let mut skill_ids: HashMap<i32, SkillIdLookup> = HashMap::new();
        let mut mission_task_uids = HashMap::new();

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
            _p: PhantomData,
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

    let rev_behaviors = rev.clone().and(warp::path("behaviors"));
    let rev_behavior_id_base = rev_behaviors.and(warp::path::param::<i32>());
    let rev_behavior_id = rev_behavior_id_base
        .and(warp::path::end())
        .map(rev_behavior_api)
        .map(map_res);

    let first = rev.clone().and(warp::path::end()).map(rev_api).map(map_res);
    first.or(rev_skill_id).unify().or(rev_behavior_id).unify()
}
