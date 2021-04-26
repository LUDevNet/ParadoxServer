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

#[derive(Debug, Copy, Clone, Default)]
pub struct Components {
    pub render: Option<i32>,
}

#[derive(Copy, Clone)]
pub(super) struct IconsTable<'db> {
    pub inner: Table<'db>,
    col_icon_path: usize,
    #[allow(dead_code)]
    col_icon_name: usize,
}

impl<'db> IconsTable<'db> {
    pub(super) fn new(inner: Table<'db>) -> Self {
        let mut col_icon_path = None;
        let mut col_icon_name = None;

        for (index, col) in inner.column_iter().enumerate() {
            match col.name_raw().as_bytes() {
                b"IconPath" => col_icon_path = Some(index),
                b"IconName" => col_icon_name = Some(index),
                _ => {}
            }
        }

        Self {
            inner,
            col_icon_path: col_icon_path.unwrap(),
            col_icon_name: col_icon_name.unwrap(),
        }
    }
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub(super) struct ItemSetsTable<'db> {
    pub inner: Table<'db>,
    /// itemIDs: ", " separated LOTs
    col_item_ids: usize,
    /// kitType i.e. faction
    col_kit_type: usize,
    /// kitRank
    col_kit_rank: usize,
    /// kitImage
    col_kit_image: usize,
}

impl<'db> ItemSetsTable<'db> {
    pub(super) fn new(inner: Table<'db>) -> Self {
        let mut item_ids = None;
        let mut kit_type = None;
        let mut kit_rank = None;
        let mut kit_image = None;

        for (index, col) in inner.column_iter().enumerate() {
            match col.name_raw().as_bytes() {
                b"itemIDs" => item_ids = Some(index),
                b"kitType" => kit_type = Some(index),
                b"kitRank" => kit_rank = Some(index),
                b"kitImage" => kit_image = Some(index),
                _ => {}
            }
        }

        Self {
            inner,
            col_item_ids: item_ids.unwrap(),
            col_kit_type: kit_type.unwrap(),
            col_kit_rank: kit_rank.unwrap(),
            col_kit_image: kit_image.unwrap(),
        }
    }

    pub(super) fn get_data(&self, id: i32) -> Option<ItemSet> {
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

#[derive(Copy, Clone)]
pub(super) struct MissionsTable<'db> {
    inner: Table<'db>,
    col_mission_icon_id: usize,
    col_is_mission: usize,
}

impl<'db> MissionsTable<'db> {
    pub(super) fn new(inner: Table<'db>) -> Self {
        let mut mission_icon_id = None;
        let mut is_mission = None;

        for (index, col) in inner.column_iter().enumerate() {
            match col.name_raw().as_bytes() {
                b"isMission" => is_mission = Some(index),
                b"missionIconID" => mission_icon_id = Some(index),
                _ => continue,
            }
        }

        Self {
            inner,
            col_mission_icon_id: mission_icon_id.unwrap(),
            col_is_mission: is_mission.unwrap(),
        }
    }
}

#[derive(Copy, Clone)]
pub(super) struct MissionTasksTable<'db> {
    inner: Table<'db>,
    col_icon_id: usize,
    col_uid: usize,
}

impl<'db> MissionTasksTable<'db> {
    pub(super) fn new(inner: Table<'db>) -> Self {
        let mut icon_id = None;
        let mut uid = None;

        for (index, col) in inner.column_iter().enumerate() {
            match col.name_raw().as_bytes() {
                b"IconID" => icon_id = Some(index),
                b"uid" => uid = Some(index),
                _ => continue,
            }
        }

        Self {
            inner,
            col_icon_id: icon_id.unwrap(),
            col_uid: uid.unwrap(),
        }
    }
}

#[derive(Copy, Clone)]
pub struct SkillBehavior {
    pub skill_icon: Option<i32>,
}

#[derive(Copy, Clone)]
pub(super) struct SkillBehaviorTable<'db> {
    inner: Table<'db>,
    col_skill_icon: usize,
}

impl<'db> SkillBehaviorTable<'db> {
    pub(super) fn new(inner: Table<'db>) -> Self {
        let mut skill_icon = None;

        for (index, col) in inner.column_iter().enumerate() {
            match col.name_raw().as_bytes() {
                b"skillIcon" => skill_icon = Some(index),
                _ => continue,
            }
        }

        Self {
            inner,
            col_skill_icon: skill_icon.unwrap(),
        }
    }

    pub(super) fn get_data(&self, id: i32) -> Option<SkillBehavior> {
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
pub(super) struct TypedDatabase<'db> {
    pub(super) locale: Arc<LocaleNode>,
    /// LU-Res Prefix
    pub(super) lu_res_prefix: &'db str,
    /// ComponentRegistry
    pub(super) comp_reg: Table<'db>,
    /// Icons
    pub(super) icons: IconsTable<'db>,
    /// ItemSets
    pub(super) item_sets: ItemSetsTable<'db>,
    /// Missions
    pub(super) missions: MissionsTable<'db>,
    /// MissionTasks
    pub(super) mission_tasks: MissionTasksTable<'db>,
    /// Objects
    pub(super) objects: Table<'db>,
    /// RenderComponent
    pub(super) render_comp: Table<'db>,
    /// SkillBehavior
    pub(super) skills: SkillBehaviorTable<'db>,
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
    pub(super) fn new(locale: Arc<LocaleNode>, lu_res_prefix: &'a str, tables: Tables<'a>) -> Self {
        TypedDatabase {
            locale,
            lu_res_prefix,
            comp_reg: tables.by_name("ComponentsRegistry").unwrap().unwrap(),
            icons: IconsTable::new(tables.by_name("Icons").unwrap().unwrap()),
            item_sets: ItemSetsTable::new(tables.by_name("ItemSets").unwrap().unwrap()),
            missions: MissionsTable::new(tables.by_name("Missions").unwrap().unwrap()),
            mission_tasks: MissionTasksTable::new(tables.by_name("MissionTasks").unwrap().unwrap()),
            objects: tables.by_name("Objects").unwrap().unwrap(),
            render_comp: tables.by_name("RenderComponent").unwrap().unwrap(),
            skills: SkillBehaviorTable::new(tables.by_name("SkillBehavior").unwrap().unwrap()),
        }
    }

    pub(super) fn get_mission_name(&self, kind: MissionKind, id: i32) -> Option<String> {
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

    pub(super) fn get_item_set_name(&self, rank: i32, id: i32) -> Option<String> {
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

    pub(super) fn get_skill_name_desc(&self, id: i32) -> (Option<String>, Option<String>) {
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

    pub(super) fn get_icon_path(&self, id: i32) -> Option<PathBuf> {
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

    pub(super) fn get_mission_data(&self, id: i32) -> Option<Mission> {
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

    pub(super) fn get_mission_tasks(&self, id: i32) -> Vec<MissionTask> {
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

    pub(super) fn get_object_name_desc(&self, id: i32) -> Option<(String, String)> {
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

    pub(super) fn get_render_image(&self, id: i32) -> Option<String> {
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

    pub(super) fn to_res_href(&self, path: &Path) -> String {
        format!("{}{}", self.lu_res_prefix, path.display())
    }

    pub(super) fn get_components(&self, id: i32) -> Components {
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
