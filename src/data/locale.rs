use std::sync::Arc;

use assembly_xml::localization::{Interner, Key, LocaleNodeRef, LocaleRoot as LocaleRootNode};
use paradox_typed_db::ext::MissionKind;

pub(crate) struct Keys {
    pub description: Key,
    pub missions: Key,
    pub mission_text: Key,
    pub mission_tasks: Key,
    pub item_sets: Key,
    pub kit_name: Key,
    pub skill_behavior: Key,
    pub description_ui: Key,
    pub name: Key,
}

impl Keys {
    fn try_new(strs: &Interner) -> Option<Self> {
        Some(Keys {
            description: strs.get("description")?,
            missions: strs.get("Missions")?,
            mission_text: strs.get("MissionText")?,
            mission_tasks: strs.get("MissionTasks")?,
            item_sets: strs.get("ItemSets")?,
            kit_name: strs.get("kitName")?,
            skill_behavior: strs.get("SkillBehavior")?,
            description_ui: strs.get("descriptionUI")?,
            name: strs.get("name")?,
        })
    }

    pub fn new(strs: &Interner) -> Self {
        Self::try_new(strs).expect("Missing some required locale keys") // FIXME: &mut Interner with intern
    }
}

pub(crate) struct LocaleRootInner {
    root: LocaleRootNode,
    /// Well known keys
    keys: Keys,
}

impl LocaleRootInner {
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn node(&self) -> LocaleNodeRef<'_, '_> {
        self.root.as_ref()
    }
}

#[derive(Clone)]
pub struct LocaleRoot {
    pub(crate) root: Arc<LocaleRootInner>,
}

impl LocaleRoot {
    pub fn new(root: LocaleRootNode) -> Self {
        Self {
            root: Arc::new(LocaleRootInner {
                keys: Keys::new(root.strs()), // FIXME: strs_mut
                root,
            }),
        }
    }

    pub fn get_mission_name(&self, kind: MissionKind, id: i32) -> Option<String> {
        let keys = &self.root.keys;
        let missions = self.root.root.as_ref().get_str(keys.missions).unwrap();
        if id > 0 {
            if let Some(mission) = missions.get_int(id as u32) {
                if let Some(name_node) = mission.get_str(keys.name) {
                    let name = name_node.value().unwrap();
                    return Some(format!("{} | {:?} #{}", name, kind, id));
                }
            }
        }
        None
    }

    pub fn get_item_set_name(&self, rank: i32, id: i32) -> Option<String> {
        let keys = &self.root.keys;
        let missions = self.root.root.as_ref().get_str(keys.item_sets).unwrap();
        if id > 0 {
            if let Some(mission) = missions.get_int(id as u32) {
                if let Some(name_node) = mission.get_str(keys.kit_name) {
                    let name = name_node.value().unwrap();
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
        let keys = &self.root.keys;
        let root = self.root.root.as_ref();
        let skills = root.get_str(keys.skill_behavior).unwrap();
        let mut the_name = None;
        let mut the_desc = None;
        if id > 0 {
            if let Some(skill) = skills.get_int(id as u32) {
                if let Some(name_node) = skill.get_str(keys.name) {
                    let name = name_node.value().unwrap();
                    the_name = Some(format!("{} | Skill #{}", name, id));
                }
                if let Some(desc_node) = skill.get_str(keys.description_ui) {
                    let desc = desc_node.value().unwrap();
                    the_desc = Some(desc.to_string());
                }
            }
        }
        (the_name, the_desc)
    }
}
