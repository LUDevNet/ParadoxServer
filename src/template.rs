use std::{
    borrow::Cow,
    convert::Infallible,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use assembly_data::{
    fdb::{
        common::{Latin1Str, Value},
        mem::{Database, Table},
    },
    xml::localization::LocaleNode,
};
use handlebars::Handlebars;
use serde::Serialize;
use warp::{path::FullPath, Filter};

pub struct WithTemplate<T: Serialize> {
    pub name: &'static str,
    pub value: T,
}

pub fn render<T>(template: WithTemplate<T>, hbs: Arc<Handlebars>) -> impl warp::Reply
where
    T: Serialize,
{
    let render = hbs
        .render(template.name, &template.value)
        .unwrap_or_else(|err| err.to_string());
    warp::reply::html(render)
}

#[derive(Serialize)]
pub struct IndexParams {
    pub title: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub r#type: &'static str,
    pub image: Cow<'static, str>,
    pub url: Cow<'static, str>,
    pub card: &'static str,
    pub site: Cow<'static, str>,
}

static DEFAULT_IMG: &str = "/ui/ingame/freetrialcongratulations_id.png";

#[derive(Debug, Copy, Clone, Default)]
struct Components {
    render: Option<i32>,
}

#[derive(Copy, Clone)]
struct IconsTable<'db> {
    inner: Table<'db>,
    col_icon_path: usize,
    #[allow(dead_code)]
    col_icon_name: usize,
}

impl<'db> IconsTable<'db> {
    fn new(inner: Table<'db>) -> Self {
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
struct MissionsTable<'db> {
    inner: Table<'db>,
    col_mission_icon_id: usize,
}

impl<'db> MissionsTable<'db> {
    fn new(inner: Table<'db>) -> Self {
        let col_mission_icon_id = inner
            .column_iter()
            .position(|c| c.name_raw().as_bytes() == b"missionIconID")
            .unwrap();
        Self {
            inner,
            col_mission_icon_id,
        }
    }
}

#[derive(Clone)]
struct TypedDatabase<'db> {
    locale: Arc<LocaleNode>,
    /// LU-Res Prefix
    lu_res_prefix: &'db str,
    /// Icons
    icons: IconsTable<'db>,
    /// Objects
    missions: MissionsTable<'db>,
    /// Objects
    objects: Table<'db>,
    /// ComponentRegistry
    comp_reg: Table<'db>,
    /// RenderComponent
    render_comp: Table<'db>,
}

#[derive(Default)]
struct Mission {
    mission_icon_id: Option<i32>,
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
    fn get_mission_name_desc(&self, id: i32) -> Option<(String, String)> {
        let missions = self.locale.str_children.get("Missions").unwrap();
        if id > 0 {
            if let Some(mission) = missions.int_children.get(&(id as u32)) {
                if let Some(name_node) = mission.str_children.get("name") {
                    let name = name_node.value.as_ref().unwrap();
                    return Some((
                        format!("{} | Mission #{} | LU-Explorer", name, id),
                        "TODO".to_string(),
                    ));
                }
            }
        }
        None
    }

    fn get_icon_path(&self, id: i32) -> Option<PathBuf> {
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

    fn get_mission_data(&self, id: i32) -> Option<Mission> {
        let hash = u32::from_ne_bytes(id.to_ne_bytes());
        let bucket = self.missions.inner.bucket_for_hash(hash);

        for row in bucket.row_iter() {
            let id_field = row.field_at(0).unwrap();

            if id_field == Value::Integer(id) {
                let mission_icon_id = row
                    .field_at(self.missions.col_mission_icon_id)
                    .unwrap()
                    .into_opt_integer();

                return Some(Mission { mission_icon_id });
            }
        }
        None
    }

    fn get_object_name_desc(&self, id: i32) -> Option<(String, String)> {
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
                        format!(
                            "{} ({}) | Object #{} | LU-Explorer",
                            display.decode(),
                            name.decode(),
                            id
                        )
                    }
                    (Some(name), _) => {
                        format!("{} | Object #{} | LU-Explorer", name.decode(), id)
                    }
                    (None, Some(display)) => {
                        format!("{} | Object #{} | LU-Explorer", display.decode(), id)
                    }
                    (None, None) => {
                        format!("Object #{} | LU-Explorer", id)
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

    fn get_render_image(&self, id: i32) -> Option<String> {
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

    fn to_res_href(&self, path: &Path) -> String {
        format!("{}{}", self.lu_res_prefix, path.display())
    }

    fn get_components(&self, id: i32) -> Components {
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

#[derive(Debug, Clone)]
struct Meta {
    title: Cow<'static, str>,
    description: Cow<'static, str>,
    image: Cow<'static, str>,
}

fn meta(
    data: TypedDatabase<'_>,
) -> impl Filter<Extract = (Meta,), Error = Infallible> + Clone + '_ {
    let mut default_img = data.lu_res_prefix.to_owned();
    default_img.push_str(DEFAULT_IMG);
    let default_img: &'static str = Box::leak(default_img.into_boxed_str());

    let base = warp::any().map(move || data.clone());

    let dashboard = warp::path("dashboard")
        .and(warp::path::end())
        .map(move || Meta {
            title: Cow::Borrowed("Dashboard | LU-Explorer"),
            description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
            image: Cow::Borrowed(default_img),
        });

    let objects_end = warp::path::end().map(move || Meta {
        title: Cow::Borrowed("Objects | LU-Explorer"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: Cow::Borrowed(default_img),
    });
    let object_get = base.clone().and(warp::path::param::<i32>()).map(
        move |data: TypedDatabase<'_>, id: i32| {
            let (title, description) = data.get_object_name_desc(id).unwrap_or((
                format!("Missing Object #{} | LU-Explorer", id),
                String::new(),
            ));
            let comp = data.get_components(id);
            let image = comp
                .render
                .and_then(|id| data.get_render_image(id))
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed(default_img));
            Meta {
                title: Cow::Owned(title),
                description: Cow::Owned(description),
                image,
            }
        },
    );
    let objects = warp::path("objects").and(objects_end.or(object_get).unify());

    let missions_end = warp::path::end().map(move || Meta {
        title: Cow::Borrowed("Missions | LU-Explorer"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: Cow::Borrowed(default_img),
    });
    let mission_get =
        base.and(warp::path::param::<i32>())
            .map(move |data: TypedDatabase<'_>, id: i32| {
                let mut image = Cow::Borrowed(default_img);
                if let Some(mission) = data.get_mission_data(id) {
                    if let Some(icon_id) = mission.mission_icon_id {
                        if let Some(path) = data.get_icon_path(icon_id) {
                            image = Cow::Owned(data.to_res_href(&path));
                        }
                    }
                }

                let (title, description) = data.get_mission_name_desc(id).unwrap_or((
                    format!("Missing Mission #{} | LU-Explorer", id),
                    String::new(),
                ));

                Meta {
                    title: Cow::Owned(title),
                    description: Cow::Owned(description),
                    image,
                }
            });
    let missions = warp::path("missions").and(missions_end.or(mission_get).unify());

    let catch = warp::any().map(move || Meta {
        title: Cow::Borrowed("LU-Explorer"),
        description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
        image: Cow::Borrowed(default_img),
    });
    objects
        .or(missions)
        .unify()
        .or(dashboard)
        .unify()
        .or(catch)
        .unify()
}

#[allow(clippy::needless_lifetimes)] // false positive?
pub fn make_spa_dynamic<'r>(
    lu_res_prefix: &'r str,
    lr: Arc<LocaleNode>,
    db: Database<'r>,
    hb: Arc<Handlebars<'r>>,
    //    hnd: ArcHandle<B, FDBHeader>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = Infallible> + Clone + 'r {
    // Find the objects table
    let tables = db.tables().unwrap();

    let data = TypedDatabase {
        locale: lr,
        lu_res_prefix,
        icons: IconsTable::new(tables.by_name("Icons").unwrap().unwrap()),
        missions: MissionsTable::new(tables.by_name("Missions").unwrap().unwrap()),
        objects: tables.by_name("Objects").unwrap().unwrap(),
        comp_reg: tables.by_name("ComponentsRegistry").unwrap().unwrap(),
        render_comp: tables.by_name("RenderComponent").unwrap().unwrap(),
    };

    // Create a reusable closure to render template
    let handlebars = move |with_template| render(with_template, hb.clone());

    warp::any()
        .and(meta(data))
        .and(warp::path::full())
        .map(|meta: Meta, full_path: FullPath| WithTemplate {
            name: "template.html",
            value: IndexParams {
                title: meta.title,
                r#type: "website",
                card: "summary",
                description: meta.description,
                site: Cow::Borrowed("@lu_explorer"),
                image: meta.image,
                url: Cow::Owned(format!("https://lu.lcdruniverse.org{}", full_path.as_str())),
            },
        })
        .map(handlebars)
}
