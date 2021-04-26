use std::{borrow::Cow, convert::Infallible, sync::Arc};

use assembly_data::{fdb::mem::Database, xml::localization::LocaleNode};
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

mod typed_db;

use typed_db::{IconsTable, MissionKind, MissionTasksTable, MissionsTable, TypedDatabase};

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
            title: Cow::Borrowed("Dashboard"),
            description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
            image: Cow::Borrowed(default_img),
        });

    let objects_end = warp::path::end().map(move || Meta {
        title: Cow::Borrowed("Objects"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: Cow::Borrowed(default_img),
    });
    let object_get = base.clone().and(warp::path::param::<i32>()).map(
        move |data: TypedDatabase<'_>, id: i32| {
            let (title, description) = data
                .get_object_name_desc(id)
                .unwrap_or((format!("Missing Object #{}", id), String::new()));
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
        title: Cow::Borrowed("Missions"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: Cow::Borrowed(default_img),
    });
    let mission_get =
        base.and(warp::path::param::<i32>())
            .map(move |data: TypedDatabase<'_>, id: i32| {
                let mut image = None;
                let mut kind = MissionKind::Mission;
                if let Some(mission) = data.get_mission_data(id) {
                    if !mission.is_mission {
                        kind = MissionKind::Achievement;
                        if let Some(icon_id) = mission.mission_icon_id {
                            if let Some(path) = data.get_icon_path(icon_id) {
                                image = Some(data.to_res_href(&path));
                            }
                        }
                    }
                }

                let mut desc = String::new();

                let tasks = data.get_mission_tasks(id);
                let tasks_locale = data.locale.str_children.get("MissionTasks").unwrap();
                for task in tasks {
                    if image == None {
                        if let Some(icon_id) = task.icon_id {
                            if let Some(path) = data.get_icon_path(icon_id) {
                                image = Some(data.to_res_href(&path));
                            }
                        }
                    }
                    if task.uid > 0 {
                        if let Some(node) = tasks_locale.int_children.get(&(task.uid as u32)) {
                            if let Some(node) = node.str_children.get("description") {
                                if let Some(string) = &node.value {
                                    desc.push_str("- ");
                                    desc.push_str(string);
                                    desc.push('\n');
                                }
                            }
                        }
                    }
                }
                if desc.ends_with('\n') {
                    desc.pop();
                }

                let title = data
                    .get_mission_name(kind, id)
                    .unwrap_or(format!("Missing {:?} #{}", kind, id));

                Meta {
                    title: Cow::Owned(title),
                    description: Cow::Owned(desc),
                    image: image.map(Cow::Owned).unwrap_or(Cow::Borrowed(default_img)),
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
        mission_tasks: MissionTasksTable::new(tables.by_name("MissionTasks").unwrap().unwrap()),
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
