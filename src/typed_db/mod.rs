use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use assembly_data::{
    fdb::{
        common::{Latin1Str, Value},
        mem::{Table, Tables},
    },
    xml::localization::LocaleNode,
};
use serde::Serialize;

pub mod typed_rows;

#[derive(Debug, Copy, Clone, Default)]
pub struct Components {
    pub render: Option<i32>,
}

pub(crate) trait TypedTable<'de> {
    fn as_table(&self) -> Table<'de>;
    fn new(inner: Table<'de>) -> Self;
}

macro_rules! make_typed {
    ($name:ident { $($(#[$meta:meta])*$col:ident $lit:literal),+ $(,)?}) => {
        #[derive(Copy, Clone)]
        #[allow(dead_code)]
        pub(crate) struct $name<'db> {
            inner: Table<'db>,
            $(pub(super) $col: usize),+
        }

        impl<'db> TypedTable<'db> for $name<'db> {
            fn as_table(&self) -> Table<'db> {
                self.inner
            }

            fn new(inner: Table<'db>) -> Self {
                $(let mut $col = None;)+

                for (index, col) in inner.column_iter().enumerate() {
                    match col.name_raw().as_bytes() {
                        $($lit => $col = Some(index),)+
                        _ => continue,
                    }
                }

                Self {
                    inner,
                    $($col: $col.unwrap(),)+
                }
            }
        }
    };
}

make_typed!(IconsTable {
    col_icon_path b"IconPath",
    col_icon_name b"IconName",
});

make_typed!(ItemSetSkillsTable {
    col_skill_set_id b"SkillSetID",
    col_skill_id b"SkillID",
    col_skill_cast_type b"SkillCastType",
});

make_typed!(ItemSetsTable {
    /// itemIDs: ", " separated LOTs
    col_item_ids b"itemIDs",
    /// kitType i.e. faction
    col_kit_type b"kitType",
    /// kitRank
    col_kit_rank b"kitRank",
    /// kitImage
    col_kit_image b"kitImage",
});

impl<'db> ItemSetsTable<'db> {
    pub(crate) fn get_data(&self, id: i32) -> Option<ItemSet> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.inner.bucket_for_hash(hash);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                let kit_type = row
                    .field_at(self.col_kit_type)
                    .unwrap()
                    .into_opt_integer()
                    .unwrap();
                let kit_rank = row
                    .field_at(self.col_kit_rank)
                    .unwrap()
                    .into_opt_integer()
                    .unwrap_or(0);
                let kit_image = row.field_at(self.col_kit_image).unwrap().into_opt_integer();
                let item_ids = row
                    .field_at(self.col_item_ids)
                    .unwrap()
                    .into_opt_text()
                    .unwrap()
                    .decode()
                    .split(',')
                    .map(str::trim)
                    .filter_map(|idstr| idstr.parse::<i32>().ok())
                    .collect();

                return Some(ItemSet {
                    kit_type,
                    kit_rank,
                    kit_image,
                    item_ids,
                });
            }
        }
        None
    }
}

make_typed!(MissionsTable {
    col_id b"id",
    col_defined_type b"defined_type",
    col_defined_subtype b"defined_subtype",
    col_ui_sort_order b"UISortOrder",
    col_is_mission b"isMission",
    col_mission_icon_id b"missionIconID",
});

make_typed!(MissionTasksTable {
    col_id b"id",
    col_loc_status b"locStatus",
    col_task_type b"taskType",
    col_target b"target",
    col_target_group b"targetGroup",
    col_target_value b"targetValue",
    col_task_param1 b"taskParam1",
    col_large_task_icon b"largeTaskIcon",
    col_icon_id b"IconID",
    col_uid b"uid",
    col_large_task_icon_id b"largeTaskIconID",
    col_localize b"localize",
    col_gate_version b"gate_version"
});

#[derive(Serialize)]
pub struct MissionTaskIcon {
    uid: i32,
    #[serde(rename = "largeTaskIconID")]
    large_task_icon_id: Option<i32>,
}

impl<'a> MissionTasksTable<'a> {
    pub fn as_task_icon_iter(&self, key: i32) -> impl Iterator<Item = MissionTaskIcon> + '_ {
        self.key_iter(key).map(|x| MissionTaskIcon {
            uid: x.uid(),
            large_task_icon_id: x.large_task_icon_id(),
        })
    }
}

make_typed!(ObjectSkillsTable {
    col_object_template b"objectTemplate",
    col_skill_id b"skillID",
    col_cast_on_type b"castOnType",
    col_ai_combat_weight b"AICombatWeight",
});

make_typed!(BehaviorParameterTable {
    col_behavior_id b"behaviorID",
    col_parameter_id b"parameterID",
    col_value b"value",
});

make_typed!(BehaviorTemplateTable {
    col_behavior_id b"behaviorID",
    col_template_id b"templateID",
    col_effect_id b"effectID",
    col_effect_handle b"effectHandle",
});

#[derive(Copy, Clone)]
pub struct SkillBehavior {
    pub skill_icon: Option<i32>,
}

make_typed!(SkillBehaviorTable {
    col_skill_id b"skillID",
    col_loc_status b"locStatus",
    col_behavior_id b"behaviorID",
    col_imaginationcost b"imaginationcost",
    col_cooldowngroup b"cooldowngroup",
    col_cooldown b"cooldown",
    col_in_npc_editor b"inNpcEditor",
    col_skill_icon b"skillIcon",
    col_oom_skill_id b"oomSkillID",
    col_oom_behavior_effect_id b"oomBehaviorEffectID",
    col_cast_type_desc b"castTypeDesc",
    col_im_bonus_ui b"imBonusUI",
    col_life_bonus_ui b"lifeBonusUI",
    col_armor_bonus_ui b"armorBonusUI",
    col_damage_ui b"damageUI",
    col_hide_icon b"hideIcon",
    col_localize b"localize",
    col_gate_version b"gate_version",
    col_cancel_type b"cancelType"
});

impl<'db> SkillBehaviorTable<'db> {
    pub(crate) fn get_data(&self, id: i32) -> Option<SkillBehavior> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.inner.bucket_for_hash(hash);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                let skill_icon = row
                    .field_at(self.col_skill_icon)
                    .unwrap()
                    .into_opt_integer();

                return Some(SkillBehavior { skill_icon });
            }
        }
        None
    }
}

#[derive(Clone)]
pub(crate) struct TypedDatabase<'db> {
    pub(crate) locale: Arc<LocaleNode>,
    /// LU-Res Prefix
    pub(crate) lu_res_prefix: &'db str,
    /// BehaviorParameter
    pub(crate) behavior_parameters: BehaviorParameterTable<'db>,
    /// BehaviorTemplate
    pub(crate) behavior_templates: BehaviorTemplateTable<'db>,
    /// ComponentRegistry
    pub(crate) comp_reg: Table<'db>,
    /// Icons
    pub(crate) icons: IconsTable<'db>,
    /// ItemSets
    pub(crate) item_sets: ItemSetsTable<'db>,
    /// ItemSetSkills
    pub(crate) item_set_skills: ItemSetSkillsTable<'db>,
    /// Missions
    pub(crate) missions: MissionsTable<'db>,
    /// MissionTasks
    pub(crate) mission_tasks: MissionTasksTable<'db>,
    /// Objects
    pub(crate) objects: Table<'db>,
    /// Objects
    pub(crate) object_skills: ObjectSkillsTable<'db>,
    /// RenderComponent
    pub(crate) render_comp: Table<'db>,
    /// SkillBehavior
    pub(crate) skills: SkillBehaviorTable<'db>,
}

#[derive(Debug, Clone)]
pub struct ItemSet {
    pub item_ids: Vec<i32>,
    pub kit_type: i32,
    pub kit_rank: i32,
    pub kit_image: Option<i32>,
}

#[derive(Default)]
pub struct Mission {
    pub mission_icon_id: Option<i32>,
    pub is_mission: bool,
}

#[derive(Default)]
pub struct MissionTask {
    pub icon_id: Option<i32>,
    pub uid: i32,
}

#[derive(Debug, Copy, Clone)]
pub enum MissionKind {
    Achievement,
    Mission,
}

fn is_not_empty(s: &&Latin1Str) -> bool {
    !s.is_empty()
}

fn cleanup_path(url: &Latin1Str) -> Option<PathBuf> {
    let url = url.decode().replace('\\', "/").to_ascii_lowercase();
    let p = Path::new(&url);

    let mut path = Path::new("/textures/ui").to_owned();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            Component::Normal(seg) => path.push(seg),
            Component::RootDir => return None,
            Component::Prefix(_) => return None,
        }
    }
    path.set_extension("png");
    Some(path)
}

impl<'a> TypedDatabase<'a> {
    pub(crate) fn new(locale: Arc<LocaleNode>, lu_res_prefix: &'a str, tables: Tables<'a>) -> Self {
        TypedDatabase {
            locale,
            lu_res_prefix,
            behavior_parameters: BehaviorParameterTable::new(
                tables.by_name("BehaviorParameter").unwrap().unwrap(),
            ),
            behavior_templates: BehaviorTemplateTable::new(
                tables.by_name("BehaviorTemplate").unwrap().unwrap(),
            ),
            comp_reg: tables.by_name("ComponentsRegistry").unwrap().unwrap(),
            icons: IconsTable::new(tables.by_name("Icons").unwrap().unwrap()),
            item_sets: ItemSetsTable::new(tables.by_name("ItemSets").unwrap().unwrap()),
            item_set_skills: ItemSetSkillsTable::new(
                tables.by_name("ItemSetSkills").unwrap().unwrap(),
            ),
            missions: MissionsTable::new(tables.by_name("Missions").unwrap().unwrap()),
            mission_tasks: MissionTasksTable::new(tables.by_name("MissionTasks").unwrap().unwrap()),
            objects: tables.by_name("Objects").unwrap().unwrap(),
            object_skills: ObjectSkillsTable::new(tables.by_name("ObjectSkills").unwrap().unwrap()),
            render_comp: tables.by_name("RenderComponent").unwrap().unwrap(),
            skills: SkillBehaviorTable::new(tables.by_name("SkillBehavior").unwrap().unwrap()),
        }
    }

    pub(crate) fn get_mission_name(&self, kind: MissionKind, id: i32) -> Option<String> {
        let missions = self.locale.str_children.get("Missions").unwrap();
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

    pub(crate) fn get_item_set_name(&self, rank: i32, id: i32) -> Option<String> {
        let missions = self.locale.str_children.get("ItemSets").unwrap();
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

    pub(crate) fn get_skill_name_desc(&self, id: i32) -> (Option<String>, Option<String>) {
        let skills = self.locale.str_children.get("SkillBehavior").unwrap();
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

    pub(crate) fn get_icon_path(&self, id: i32) -> Option<PathBuf> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.icons.inner.bucket_for_hash(hash);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                if let Some(url) = row
                    .field_at(self.icons.col_icon_path)
                    .unwrap()
                    .into_opt_text()
                {
                    return cleanup_path(url);
                }
            }
        }
        None
    }

    pub(crate) fn get_mission_data(&self, id: i32) -> Option<Mission> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.missions.inner.bucket_for_hash(hash);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                let mission_icon_id = row
                    .field_at(self.missions.col_mission_icon_id)
                    .unwrap()
                    .into_opt_integer();
                let is_mission = row
                    .field_at(self.missions.col_is_mission)
                    .unwrap()
                    .into_opt_boolean()
                    .unwrap_or(true);

                return Some(Mission {
                    mission_icon_id,
                    is_mission,
                });
            }
        }
        None
    }

    pub(crate) fn get_mission_tasks(&self, id: i32) -> Vec<MissionTask> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.mission_tasks.inner.bucket_for_hash(hash);
        let mut tasks = Vec::with_capacity(4);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                let icon_id = row
                    .field_at(self.mission_tasks.col_icon_id)
                    .unwrap()
                    .into_opt_integer();
                let uid = row
                    .field_at(self.mission_tasks.col_uid)
                    .unwrap()
                    .into_opt_integer()
                    .unwrap();

                tasks.push(MissionTask { icon_id, uid })
            }
        }
        tasks
    }

    pub(crate) fn get_object_name_desc(&self, id: i32) -> Option<(String, String)> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self
            .objects
            .bucket_at(hash as usize % self.objects.bucket_count())
            .unwrap();

        for row in bucket.row_iter() {
            let mut fields = row.field_iter();
            let id_field = fields.next().unwrap();
            if id_field == Value::Integer(id) {
                let name = fields.next().unwrap(); // 1: name
                let description = fields.nth(2).unwrap(); // 4: description
                let display_name = fields.nth(2).unwrap(); // 7: displayName
                let internal_notes = fields.nth(2).unwrap(); // 10: internalNotes

                let title = match (
                    name.into_opt_text().filter(is_not_empty),
                    display_name.into_opt_text().filter(is_not_empty),
                ) {
                    (Some(name), Some(display)) if display != name => {
                        format!("{} ({}) | Object #{}", display.decode(), name.decode(), id)
                    }
                    (Some(name), _) => {
                        format!("{} | Object #{}", name.decode(), id)
                    }
                    (None, Some(display)) => {
                        format!("{} | Object #{}", display.decode(), id)
                    }
                    (None, None) => {
                        format!("Object #{}", id)
                    }
                };
                let desc = match (
                    description.into_opt_text().filter(is_not_empty),
                    internal_notes.into_opt_text().filter(is_not_empty),
                ) {
                    (Some(description), Some(internal_notes)) if description != internal_notes => {
                        format!("{} ({})", description.decode(), internal_notes.decode(),)
                    }
                    (Some(description), _) => {
                        format!("{}", description.decode())
                    }
                    (None, Some(internal_notes)) => {
                        format!("{}", internal_notes.decode())
                    }
                    (None, None) => String::new(),
                };
                return Some((title, desc));
            }
        }
        None
    }

    pub(crate) fn get_render_image(&self, id: i32) -> Option<String> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self
            .render_comp
            .bucket_at(hash as usize % self.render_comp.bucket_count())
            .unwrap();

        for row in bucket.row_iter() {
            let mut fields = row.field_iter();
            let id_field = fields.next().unwrap();
            if id_field == Value::Integer(id) {
                let _render_asset = fields.next().unwrap();
                let icon_asset = fields.next().unwrap();

                if let Value::Text(url) = icon_asset {
                    let path = cleanup_path(url)?;
                    return Some(self.to_res_href(&path));
                }
            }
        }
        None
    }

    pub(crate) fn to_res_href(&self, path: &Path) -> String {
        format!("{}{}", self.lu_res_prefix, path.display())
    }

    pub(crate) fn get_components(&self, id: i32) -> Components {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self
            .comp_reg
            .bucket_at(hash as usize % self.comp_reg.bucket_count())
            .unwrap();

        let mut comp = Components::default();

        for row in bucket.row_iter() {
            let mut fields = row.field_iter();
            let id_field = fields.next().unwrap();
            if id_field == Value::Integer(id) {
                let component_type = fields.next().unwrap();
                let component_id = fields.next().unwrap();

                if let Value::Integer(2) = component_type {
                    comp.render = component_id.into_opt_integer();
                }
            }
        }
        comp
    }
}
