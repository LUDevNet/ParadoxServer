use std::{borrow::Borrow, collections::BTreeMap, convert::Infallible};

use assembly_core::buffer::CastError;
use assembly_data::xml::localization::LocaleNode;
use paradox_typed_db::{typed_rows::MissionsRow, TypedDatabase};
use serde::{ser::SerializeMap, Serialize};
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use crate::api::{
    adapter::{I32Slice, IdentityHash, LocaleTableAdapter, TypedTableIterAdapter},
    map_res, PercentDecoded,
};

use super::{common::MissionsTaskIconsAdapter, Api, Rev};

#[derive(Debug, Clone)]
struct MissionSubtypesAdapter<'a>(&'a BTreeMap<String, Vec<i32>>);

impl<'a> Serialize for MissionSubtypesAdapter<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.0.keys())
    }
}

#[derive(Debug, Clone)]
struct MissionTypesAdapter<'a>(&'a BTreeMap<String, BTreeMap<String, Vec<i32>>>);

impl<'a> Serialize for MissionTypesAdapter<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            m.serialize_entry(key, &MissionSubtypesAdapter(value))?;
        }
        m.end()
    }
}

fn rev_mission_types_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    Ok(warp::reply::json(&MissionTypesAdapter(
        &rev.inner.mission_types,
    )))
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

type MissionsAdapter<'a, 'b> =
    TypedTableIterAdapter<'a, 'b, MissionsRow<'a, 'b>, IdentityHash, I32Slice<'b>>;

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
    subtypes: MissionSubtypesAdapter<'a>,
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
                subtypes: MissionSubtypesAdapter(t),
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

pub(super) fn mission_types_api<
    F: Filter<Extract = super::Ext, Error = Infallible> + Send + Sync + Clone + 'static,
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
