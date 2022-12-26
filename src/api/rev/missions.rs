use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
};

use assembly_xml::localization::LocaleNode;
use paradox_typed_db::{rows::MissionsRow, TypedDatabase};
use serde::{ser::SerializeMap, Serialize};

use super::{
    data::{ComponentUse, MissionRev, COMPONENT_ID_COLLECTIBLE, COMPONENT_ID_ITEM},
    ReverseLookup,
};
use crate::{
    api::{
        adapter::{Filtered, I32Slice, IdentityHash, LocaleTableAdapter, TypedTableIterAdapter},
        PercentDecoded,
    },
    data::locale::LocaleRoot,
};

use super::{common::MissionsTaskIconsAdapter, Api};

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
pub(super) struct MissionTypesAdapter<'a>(&'a BTreeMap<String, BTreeMap<String, Vec<i32>>>);

impl<'a> MissionTypesAdapter<'a> {
    pub fn new(rev: &'a ReverseLookup) -> Self {
        Self(&rev.mission_types)
    }
}

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
pub(super) struct MissionTypesEmbedded<'a, 'b> {
    #[serde(rename = "Missions")]
    missions: MissionsAdapter<'a, 'b>,
    #[serde(rename = "MissionTaskIcons")]
    mission_task_icons: MissionsTaskIconsAdapter<'a, 'b>,
    locale: MissionLocale<'b>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Subtypes<'a> {
    subtypes: MissionSubtypesAdapter<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct MissionIDList<'b> {
    mission_ids: &'b [i32],
}

type MissionsReply<'a, 'b> = Api<MissionIDList<'b>, MissionTypesEmbedded<'a, 'b>>;
fn missions_reply<'a, 'b>(
    db: &'b TypedDatabase<'a>,
    loc: &'b LocaleRoot,
    mission_ids: &'b [i32],
) -> MissionsReply<'a, 'b> {
    Api {
        data: MissionIDList { mission_ids },
        embedded: MissionTypesEmbedded {
            missions: MissionsAdapter::new(&db.missions, mission_ids),
            mission_task_icons: MissionsTaskIconsAdapter::new(&db.mission_tasks, mission_ids),
            locale: MissionLocale::new(&loc.root, mission_ids),
        },
    }
}

pub(super) enum RevMissionTypeReply<'a, 'b> {
    Subtypes(Subtypes<'a>),
    Missions(MissionsReply<'a, 'b>),
    None,
}

impl<'a, 'b> Serialize for RevMissionTypeReply<'a, 'b> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Subtypes(s) => s.serialize(serializer),
            Self::Missions(m) => m.serialize(serializer),
            Self::None => serializer.serialize_none(),
        }
    }
}

pub(super) fn rev_mission_type<'a, 'b: 'a>(
    db: &'b TypedDatabase<'a>,
    rev: &'b ReverseLookup,
    loc: &'b LocaleRoot,
    d_type: PercentDecoded,
) -> RevMissionTypeReply<'a, 'b> {
    let key: &String = d_type.borrow();
    match rev.mission_types.get(key) {
        Some(t) => match t.get("") {
            Some(mission_ids) => {
                RevMissionTypeReply::Missions(missions_reply(db, loc, mission_ids))
            }
            None => RevMissionTypeReply::Subtypes(Subtypes {
                subtypes: MissionSubtypesAdapter(t),
            }),
        },
        None => RevMissionTypeReply::None,
    }
}

pub(super) fn rev_mission_subtype<'a, 'b>(
    db: &'b TypedDatabase<'a>,
    rev: &'b ReverseLookup,
    loc: &'b LocaleRoot,
    d_type: PercentDecoded,
    d_subtype: PercentDecoded,
) -> Option<MissionsReply<'a, 'b>> {
    let t_key: &String = d_type.borrow();
    let t = rev.mission_types.get(t_key)?;
    let s_key: &String = d_subtype.borrow();
    let mission_ids = t.get(s_key)?;
    Some(missions_reply(db, loc, mission_ids))
}

#[derive(Serialize)]
pub struct MissionByIdEmbedded {
    #[serde(rename = "ItemComponent")]
    item_components: Filtered<BTreeMap<i32, ComponentUse>, &'static BTreeSet<i32>>,
    #[serde(rename = "CollectibleComponent")]
    collectible_components: Filtered<BTreeMap<i32, ComponentUse>, &'static BTreeSet<i32>>,
}

pub(crate) fn mission_by_id(
    rev: &'static ReverseLookup,
    id: i32,
) -> Option<Api<&'static MissionRev, MissionByIdEmbedded>> {
    rev.missions.get(&id).map(|data| Api {
        data,
        embedded: MissionByIdEmbedded {
            item_components: rev
                .component_use
                .filter(COMPONENT_ID_ITEM, &data.item_components.requirement_for)
                .unwrap(),
            collectible_components: rev
                .component_use
                .filter(
                    COMPONENT_ID_COLLECTIBLE,
                    &data.collectible_components.requirement_for,
                )
                .unwrap(),
        },
    })
}
