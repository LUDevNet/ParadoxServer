use std::sync::Arc;

use assembly_xml::localization::{Key, LocaleNode};
use once_cell::sync::Lazy;
use paradox_typed_db::ext::MissionKind;

#[derive(Clone)]
pub struct LocaleRoot {
    pub root: Arc<LocaleNode>,
}

struct Keys {
    missions: Key,
    item_sets: Key,
    kit_name: Key,
    skill_behavior: Key,
    description_ui: Key,
    name: Key,
}

static KEYS: Lazy<Keys> = Lazy::new(|| Keys {
    missions: Key::from_str("Missions").unwrap(),
    item_sets: Key::from_str("ItemSets").unwrap(),
    kit_name: Key::from_str("kitName").unwrap(),
    skill_behavior: Key::from_str("SkillBehavior").unwrap(),
    description_ui: Key::from_str("descriptionUI").unwrap(),
    name: Key::from_str("name").unwrap(),
});

impl LocaleRoot {
    pub fn new(root_node: LocaleNode) -> Self {
        Self {
            root: Arc::new(root_node),
        }
    }

    pub fn get_mission_name(&self, kind: MissionKind, id: i32) -> Option<String> {
        let missions = self.root.str_children.get(&KEYS.missions).unwrap();
        if id > 0 {
            if let Some(mission) = missions.int_children.get(&(id as u32)) {
                if let Some(name_node) = mission.str_children.get(&KEYS.name) {
                    let name = name_node.value.as_ref().unwrap();
                    return Some(format!("{} | {:?} #{}", name, kind, id));
                }
            }
        }
        None
    }

    pub fn get_item_set_name(&self, rank: i32, id: i32) -> Option<String> {
        let missions = self.root.str_children.get(&KEYS.item_sets).unwrap();
        if id > 0 {
            if let Some(mission) = missions.int_children.get(&(id as u32)) {
                if let Some(name_node) = mission.str_children.get(&KEYS.kit_name) {
                    let name = name_node.value.as_ref().unwrap();
                    return Some(if rank > 0 {
                        format!("{} (Rank {}) | Item Set #{}", name, rank, id)
                    } else {
                        format!("{} | Item Set #{}", name, id)
                    });
                }
            }
        }
        None
    }

    pub fn get_skill_name_desc(&self, id: i32) -> (Option<String>, Option<String>) {
        let skills = self.root.str_children.get(&KEYS.skill_behavior).unwrap();
        let mut the_name = None;
        let mut the_desc = None;
        if id > 0 {
            if let Some(skill) = skills.int_children.get(&(id as u32)) {
                if let Some(name_node) = skill.str_children.get(&KEYS.name) {
                    let name = name_node.value.as_ref().unwrap();
                    the_name = Some(format!("{} | Skill #{}", name, id));
                }
                if let Some(desc_node) = skill.str_children.get(&KEYS.description_ui) {
                    let desc = desc_node.value.as_ref().unwrap();
                    the_desc = Some(desc.clone());
                }
            }
        }
        (the_name, the_desc)
    }
}
