use std::sync::Arc;

use assembly_xml::localization::LocaleNode;
use paradox_typed_db::ext::MissionKind;

#[derive(Clone)]
pub struct LocaleRoot {
    pub root: Arc<LocaleNode>,
}

impl LocaleRoot {
    pub fn new(root: Arc<LocaleNode>) -> Self {
        Self { root }
    }

    pub fn get_mission_name(&self, kind: MissionKind, id: i32) -> Option<String> {
        let missions = self.root.str_children.get("Missions").unwrap();
        if id > 0 {
            if let Some(mission) = missions.int_children.get(&(id as u32)) {
                if let Some(name_node) = mission.str_children.get("name") {
                    let name = name_node.value.as_ref().unwrap();
                    return Some(format!("{} | {:?} #{}", name, kind, id));
                }
            }
        }
        None
    }

    pub fn get_item_set_name(&self, rank: i32, id: i32) -> Option<String> {
        let missions = self.root.str_children.get("ItemSets").unwrap();
        if id > 0 {
            if let Some(mission) = missions.int_children.get(&(id as u32)) {
                if let Some(name_node) = mission.str_children.get("kitName") {
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
        let skills = self.root.str_children.get("SkillBehavior").unwrap();
        let mut the_name = None;
        let mut the_desc = None;
        if id > 0 {
            if let Some(skill) = skills.int_children.get(&(id as u32)) {
                if let Some(name_node) = skill.str_children.get("name") {
                    let name = name_node.value.as_ref().unwrap();
                    the_name = Some(format!("{} | Item Set #{}", name, id));
                }
                if let Some(desc_node) = skill.str_children.get("descriptionUI") {
                    let desc = desc_node.value.as_ref().unwrap();
                    the_desc = Some(desc.clone());
                }
            }
        }
        (the_name, the_desc)
    }
}
