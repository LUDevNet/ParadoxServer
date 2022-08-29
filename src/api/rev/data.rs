//! # Index structures
//!
//! This module contains reverse index structures. It uses an in memory FDB
//! instance to create an (owned) `ReverseLookup` struct. This struct can then
//! be used to access the data in the FDB - potentially faster than scanning
//! it on every request.
//!
//! The [`ReverseLookup::new`] function is called once at startup of the server
//! and the result is passed to the API filters.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
    str::FromStr,
    time::Instant,
};

use assembly_fdb::common::Latin1Str;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use tracing::info;

use crate::{api::adapter::Keys, data::skill_system::match_action_key};

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

#[derive(Debug, Default, Clone, Serialize)]
pub struct BehaviorKeyIndex {
    skill: BTreeSet<i32>,
    uses: BTreeSet<i32>,
    used_by: BTreeSet<i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ComponentUse {
    pub lots: Vec<i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ComponentsUse {
    /// Map from component_id to list of object_id
    pub components: BTreeMap<i32, ComponentUse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionTaskUIDLookup {
    pub mission: i32,
}

#[derive(Debug, Default, Clone, Serialize)]
/// All data associated with a specific activity ID
pub struct ActivityRev {
    /// IDs of the RebuildComponent with matching `activityID`
    rebuild: Vec<i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LootTableIndexRev {
    /// This is a map from `LootTable::id` to `LootTable::itemid` for the current LootTableIndex
    pub items: BTreeMap<i32, i32>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct FactionRev {
    /// DestructibleComponents have the current ID in `factionList`
    pub destructible_list: Vec<i32>,
    /// DestructibleComponents have the current ID in `faction`
    pub destructible: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectStrings {
    /// name
    n: String,
    /// description
    #[serde(skip_serializing_if = "Option::is_none")]
    d: Option<String>,
    /// display_name
    #[serde(skip_serializing_if = "Option::is_none")]
    i: Option<String>,
    /// internal_notes
    #[serde(skip_serializing_if = "Option::is_none")]
    t: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectItemComponentUse {
    currency_lot: BTreeSet<i32>,
    commendation_lot: BTreeSet<i32>,
    subitems: BTreeSet<i32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectJetPackUse {
    lot_blocker: BTreeSet<i32>,
    lot_warning_volume: BTreeSet<i32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectMissionUse {
    reward_items: BTreeSet<i32>,
    // ignore offer, target for now, should be inverse to MissionNPCComponent
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectPetTamingUse {
    model_lot: BTreeSet<i32>,
    npc_lot: BTreeSet<i32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectsUse {
    /// The `CurrencyDenominations.value` matching this LOT
    currency_denomination: Option<i32>,
    deletion_restrictions: BTreeSet<i32>,
    inventory_component: BTreeSet<i32>,
    item_component: ObjectItemComponentUse,
    item_sets: BTreeSet<i32>,
    jet_pack_pad_component: ObjectJetPackUse,
    //loot_table: BTreeSet<i32>, // <- primary key here
    npc_icons_lot: BTreeSet<i32>,
    rebuild_sections: BTreeSet<i32>,
    missions: ObjectMissionUse,
    reward_codes: BTreeSet<i32>,
    pet_taming_puzzles: ObjectPetTamingUse,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ObjectsRevData {
    pub search_index: BTreeMap<i32, ObjectStrings>,
    pub rev: BTreeMap<i32, ObjectsUse>,
}

impl ObjectsRevData {
    fn r(&mut self, lot: i32) -> &mut ObjectsUse {
        self.rev.entry(lot).or_default()
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GateVersionUse {
    activities: BTreeSet<i32>,
    deletion_restrictions: BTreeSet<i32>,
    emotes: BTreeSet<i32>,
    loot_matrix: BTreeSet<i32>,
    item_sets: BTreeSet<i32>,
    missions: BTreeSet<i32>,
    mission_tasks: BTreeSet<i32>,
    objects: BTreeSet<i32>,
    player_statistics: BTreeSet<i32>,
    preconditions: BTreeSet<i32>,
    property_template: BTreeSet<i32>,
    reward_codes: BTreeSet<i32>,
    speedchat_menu: BTreeSet<i32>,
    skills: BTreeSet<i32>,
    ug_behavior_sounds: BTreeSet<i32>,
    whats_cool_item_spotlight: BTreeSet<i32>,
    whats_cool_news_and_tips: BTreeSet<i32>,
    zone_loading_tips: BTreeSet<i32>,
    zones: BTreeSet<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct GateVersionsUse {
    inner: BTreeMap<String, GateVersionUse>,
}

impl GateVersionsUse {
    fn get_or_default(&mut self, key: &Latin1Str) -> &mut GateVersionUse {
        let str_key = key.decode();
        if self.inner.contains_key(str_key.as_ref()) {
            self.inner.get_mut(str_key.as_ref()).unwrap()
        } else {
            self.inner.entry(str_key.into_owned()).or_default()
        }
    }

    pub(crate) fn keys(&self) -> Keys<&BTreeMap<String, GateVersionUse>> {
        Keys::new(&self.inner)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&GateVersionUse> {
        self.inner.get(name)
    }
}

impl serde::Serialize for GateVersionsUse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReverseLookup {
    pub mission_task_uids: HashMap<i32, MissionTaskUIDLookup>,
    pub skill_ids: HashMap<i32, SkillIdLookup>,
    pub behaviors: BTreeMap<i32, BehaviorKeyIndex>,
    pub mission_types: BTreeMap<String, BTreeMap<String, Vec<i32>>>,

    pub objects: ObjectsRevData,
    pub object_types: BTreeMap<String, Vec<i32>>,
    pub component_use: BTreeMap<i32, ComponentsUse>,
    pub activities: BTreeMap<i32, ActivityRev>,
    pub loot_table_index: BTreeMap<i32, LootTableIndexRev>,
    pub gate_versions: GateVersionsUse,
}

impl ReverseLookup {
    pub(crate) fn new(db: &'_ TypedDatabase<'_>) -> Self {
        let time = Instant::now();
        info!("Starting to load ReverseLookup");
        let mut skill_ids: HashMap<i32, SkillIdLookup> = HashMap::new();
        let mut mission_task_uids = HashMap::new();
        let mut mission_types: BTreeMap<String, BTreeMap<String, Vec<i32>>> = BTreeMap::new();
        let mut gate_versions = GateVersionsUse::default();
        let mut objects = ObjectsRevData::default();

        for a in db.activities.row_iter() {
            let id = a.activity_id();
            if let Some(gate) = a.gate_version() {
                gate_versions.get_or_default(gate).activities.insert(id);
            }
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

        let mut component_use: BTreeMap<i32, ComponentsUse> = BTreeMap::new();
        for creg in db.comp_reg.row_iter() {
            let id = creg.id();
            let ty = creg.component_type();
            let cid = creg.component_id();
            let ty_entry = component_use.entry(ty).or_default();
            let co_entry = ty_entry.components.entry(cid).or_default();
            co_entry.lots.push(id);
        }

        for row in db.currency_denominations.row_iter() {
            objects
                .rev
                .entry(row.objectid())
                .or_default()
                .currency_denomination = Some(row.value());
        }

        for row in db.deletion_restrictions.row_iter() {
            let id = row.id();
            if row.check_type() == 0 {
                if let Some(ids) = row.ids() {
                    let s = ids.decode();
                    for id_str in s.as_ref().trim().split(',').map(str::trim) {
                        if let Ok(lot) = id_str.parse() {
                            objects
                                .rev
                                .entry(lot)
                                .or_default()
                                .deletion_restrictions
                                .insert(id);
                        }
                    }
                }
            }
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .deletion_restrictions
                    .insert(id);
            }
        }

        let mut factions: BTreeMap<i32, FactionRev> = BTreeMap::new();
        for d in db.destructible_component.row_iter() {
            if let Some(faction) = d.faction() {
                let entry = factions.entry(faction).or_default();
                entry.destructible.push(d.id());
            }

            let faction_list: i32 = d.faction_list().decode().parse().unwrap();
            if faction_list >= 0 {
                let entry = factions.entry(faction_list).or_default();
                entry.destructible_list.push(d.id());
            }
        }

        for row in db.emotes.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).emotes.insert(id);
            }
        }

        for row in db.loot_matrix.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).loot_matrix.insert(id);
            }
        }

        for row in db.inventory_component.row_iter() {
            objects.r(row.itemid()).inventory_component.insert(row.id());
        }

        for row in db.item_component.row_iter() {
            let id = row.id();
            if let Some(lot) = row.currency_lot() {
                objects.r(lot).item_component.currency_lot.insert(id);
            }
            if let Some(lot) = row.commendation_lot() {
                objects.r(lot).item_component.commendation_lot.insert(id);
            }
            if let Some(text) = row.sub_items() {
                for lot in text
                    .decode()
                    .trim()
                    .split(',')
                    .map(str::trim)
                    .map(FromStr::from_str)
                    .filter_map(Result::ok)
                {
                    objects.r(lot).item_component.subitems.insert(id);
                }
            }
        }

        for s in db.item_set_skills.row_iter() {
            skill_ids
                .entry(s.skill_id())
                .or_default()
                .item_sets
                .push(s.skill_set_id());
        }

        for item_set in db.item_sets.row_iter() {
            let set_id = item_set.set_id();
            if let Some(gate_version) = item_set.gate_version() {
                gate_versions
                    .get_or_default(gate_version)
                    .item_sets
                    .insert(set_id);
            }

            for lot in item_set
                .item_i_ds()
                .decode()
                .trim()
                .split(',')
                .map(str::trim)
                .map(FromStr::from_str)
                .filter_map(Result::ok)
            {
                objects.r(lot).item_sets.insert(set_id);
            }
        }

        for row in db.jet_pack_pad_component.row_iter() {
            let id = row.id();
            if let Some(lot) = row.lot_warning_volume() {
                objects
                    .r(lot)
                    .jet_pack_pad_component
                    .lot_warning_volume
                    .insert(id);
            }
            if let Some(lot) = row.lot_blocker() {
                objects.r(lot).jet_pack_pad_component.lot_blocker.insert(id);
            }
        }

        let mut loot_table_index: BTreeMap<i32, LootTableIndexRev> = BTreeMap::new();
        for l in db.loot_table.row_iter() {
            let lti = l.loot_table_index();
            let itemid = l.itemid();
            let id = l.id();
            let entry = loot_table_index.entry(lti).or_default();
            entry.items.insert(id, itemid);
        }

        for m in db.missions.row_iter() {
            let id = m.id();
            let d_type = m.defined_type().decode().into_owned();
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
                .push(id);

            if let Some(gate_version) = m.gate_version() {
                gate_versions
                    .get_or_default(gate_version)
                    .missions
                    .insert(id);
            }

            for lot in [
                m.reward_item1(),
                m.reward_item2(),
                m.reward_item3(),
                m.reward_item4(),
                m.reward_item1_repeatable(),
                m.reward_item2_repeatable(),
                m.reward_item3_repeatable(),
                m.reward_item4_repeatable(),
            ] {
                if lot > 0 {
                    objects.r(lot).missions.reward_items.insert(id);
                }
            }
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

            if let Some(gate_version) = r.gate_version() {
                gate_versions
                    .get_or_default(gate_version)
                    .mission_tasks
                    .insert(uid);
            }

            //skill_ids.entry(r.uid()).or_default().mission_tasks.push(r
        }

        for row in db.npc_icons.row_iter() {
            let id = row.id();
            let lot = row.lot();
            objects.r(lot).npc_icons_lot.insert(id);
        }

        for s in db.object_skills.row_iter() {
            skill_ids
                .entry(s.skill_id())
                .or_default()
                .objects
                .push(s.object_template());
        }

        let mut object_types = BTreeMap::<_, Vec<_>>::new();
        for o in db.objects.row_iter() {
            let id = o.id();
            let ty = o.r#type().decode().into_owned();

            let entry = object_types.entry(ty).or_default();
            entry.push(id);

            let name = o.name().decode().into_owned();
            let description = o.description().map(Latin1Str::decode).map(Cow::into_owned);
            let display_name = o.display_name().map(Latin1Str::decode).map(Cow::into_owned);
            let internal_notes = o
                .internal_notes()
                .map(Latin1Str::decode)
                .map(Cow::into_owned);

            objects.search_index.insert(
                id,
                ObjectStrings {
                    n: name,
                    d: description,
                    i: display_name,
                    t: internal_notes,
                },
            );

            if let Some(gate_version) = o.gate_version() {
                gate_versions
                    .get_or_default(gate_version)
                    .objects
                    .insert(id);
            }
        }

        for row in db.player_statistics.row_iter() {
            let id = row.stat_id();
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .player_statistics
                    .insert(id);
            }
        }

        for row in db.preconditions.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).preconditions.insert(id);
            }
        }

        for row in db.property_template.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .property_template
                    .insert(id);
            }
        }

        let mut activities: BTreeMap<i32, ActivityRev> = BTreeMap::new();
        for r in db.rebuild_component.row_iter() {
            let id = r.id();
            if let Some(aid) = r.activity_id() {
                let entry = activities.entry(aid).or_default();
                entry.rebuild.push(id);
            }
        }

        for row in db.rebuild_sections.row_iter() {
            let id = row.id();
            let lot = row.object_id();
            objects.r(lot).rebuild_sections.insert(id);
        }

        for row in db.reward_codes.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).reward_codes.insert(id);
            }
            if let Some(lot) = row.attachment_lot() {
                objects.r(lot).reward_codes.insert(id);
            }
        }

        for skill in db.skills.row_iter() {
            let skid = skill.skill_id();
            let bid = skill.behavior_id();
            behaviors.entry(bid).or_default().skill.insert(skid);

            if let Some(gate_version) = skill.gate_version() {
                gate_versions
                    .get_or_default(gate_version)
                    .skills
                    .insert(skid);
            }
        }

        for row in db.speedchat_menu.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).speedchat_menu.insert(id);
            }
        }

        for row in db.ug_behavior_sounds.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .ug_behavior_sounds
                    .insert(id);
            }
        }

        for row in db.whats_cool_item_spotlight.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .whats_cool_item_spotlight
                    .insert(id);
            }
        }

        for row in db.whats_cool_news_and_tips.row_iter() {
            let id = row.id();
            if let Some(gate) = row.gate_version() {
                gate_versions
                    .get_or_default(gate)
                    .whats_cool_news_and_tips
                    .insert(id);
            }
        }

        for row in db.zone_loading_tips.row_iter() {
            let id = row.id();
            let gate = row.gate_version();
            gate_versions
                .get_or_default(gate)
                .zone_loading_tips
                .insert(id);
        }

        for row in db.zone_table.row_iter() {
            let id = row.zone_id();
            if let Some(gate) = row.gate_version() {
                gate_versions.get_or_default(gate).zones.insert(id);
            }
        }

        let duration = time.elapsed();
        info!("Done loading ReverseLookup ({}ms)", duration.as_millis());
        Self {
            behaviors,
            skill_ids,
            mission_task_uids,
            mission_types,

            objects,
            object_types,
            component_use,
            activities,
            loot_table_index,
            gate_versions,
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
