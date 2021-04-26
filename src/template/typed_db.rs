use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use assembly_data::{
    fdb::{
        common::{Latin1Str, Value},
        mem::Table,
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

#[derive(Clone)]
pub(super) struct TypedDatabase<'db> {
    pub(super) locale: Arc<LocaleNode>,
    /// LU-Res Prefix
    pub(super) lu_res_prefix: &'db str,
    /// Icons
    pub(super) icons: IconsTable<'db>,
    /// Missions
    pub(super) missions: MissionsTable<'db>,
    /// MissionTasks
    pub(super) mission_tasks: MissionTasksTable<'db>,
    /// Objects
    pub(super) objects: Table<'db>,
    /// ComponentRegistry
    pub(super) comp_reg: Table<'db>,
    /// RenderComponent
    pub(super) render_comp: Table<'db>,
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

impl TypedDatabase<'_> {
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
