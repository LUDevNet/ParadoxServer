use std::{borrow::Cow, convert::Infallible, fmt::Write, sync::Arc};

//use assembly_data::{fdb::mem::Database, xml::localization::LocaleNode};
use handlebars::Handlebars;
use paradox_typed_db::{typed_ext::MissionKind, TypedDatabase};
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

/// Retrieve metadata for /missions/:id
fn mission_get_impl(data: &'_ TypedDatabase<'_>, id: i32) -> Meta {
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
        image,
    }
}

/// Retrieve metadata for /objects/:id
fn object_get_api(data: &'_ TypedDatabase<'_>, id: i32) -> Meta {
    let (title, description) = data
        .get_object_name_desc(id)
        .unwrap_or((format!("Missing Object #{}", id), String::new()));
    let comp = data.get_components(id);
    let image = comp.render.and_then(|id| data.get_render_image(id));
    Meta {
        title: Cow::Owned(title),
        description: Cow::Owned(description),
        image,
    }
}

/// Retrieve metadata for /objects/item-sets/:id
fn item_set_get_impl(data: &'_ TypedDatabase<'_>, id: i32) -> Meta {
    let mut rank = 0;
    let mut image = None;
    let mut desc = String::new();
    if let Some(item_set) = data.item_sets.get_data(id) {
        rank = item_set.kit_rank;
        if let Some(image_id) = item_set.kit_image {
            if let Some(path) = data.get_icon_path(image_id) {
                image = Some(data.to_res_href(&path));
            }
        }

        for item_id in item_set.item_ids {
            if let Some((name, _)) = data.get_object_name_desc(item_id) {
                writeln!(desc, "- {}", name).unwrap();
            }
        }
    }

    if desc.ends_with('\n') {
        desc.pop();
    }

    let title = data
        .get_item_set_name(rank, id)
        .unwrap_or(format!("Unnamed Item Set #{}", id));

    Meta {
        title: Cow::Owned(title),
        description: Cow::Owned(desc),
        image,
    }
}

/// Retrieve metadata for /skills/:id
fn skill_get_impl(data: &'_ TypedDatabase<'_>, id: i32) -> Meta {
    let (mut title, description) = data.get_skill_name_desc(id);
    let description = description.map(Cow::Owned).unwrap_or(Cow::Borrowed(""));
    let mut image = None;

    if let Some(skill) = data.skills.get_data(id) {
        if title.is_none() {
            title = Some(format!("Skill #{}", id))
        }
        if let Some(icon_id) = skill.skill_icon {
            if let Some(path) = data.get_icon_path(icon_id) {
                image = Some(data.to_res_href(&path));
            }
        }
    }

    let title = title.unwrap_or(format!("Missing Skill #{}", id));

    Meta {
        title: Cow::Owned(title),
        description,
        image,
    }
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

#[derive(Debug, Clone)]
struct Meta {
    title: Cow<'static, str>,
    description: Cow<'static, str>,
    image: Option<String>,
}

fn meta<'r>(
    data: &'static TypedDatabase<'static>,
) -> impl Filter<Extract = (Meta,), Error = Infallible> + Clone + 'r {
    let base = warp::any().map(move || data);

    let dashboard = warp::path("dashboard").and(warp::path::end()).map(|| Meta {
        title: Cow::Borrowed("Dashboard"),
        description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
        image: None,
    });

    let objects_end = warp::path::end().map(|| Meta {
        title: Cow::Borrowed("Objects"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: None,
    });
    let object_get = base.and(warp::path::param::<i32>()).map(object_get_api);
    let item_sets_end = warp::path::end().map(|| Meta {
        title: Cow::Borrowed("Item Sets"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: None,
    });
    let item_set_get = base.and(warp::path::param::<i32>()).map(item_set_get_impl);
    let item_sets = warp::path("item-sets").and(item_sets_end.or(item_set_get).unify());
    let objects =
        warp::path("objects").and(objects_end.or(object_get).unify().or(item_sets).unify());

    let missions_end = warp::path::end().map(move || Meta {
        title: Cow::Borrowed("Missions"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: None,
    });
    let mission_get = base.and(warp::path::param::<i32>()).map(mission_get_impl);
    let missions = warp::path("missions").and(missions_end.or(mission_get).unify());

    let skills_end = warp::path::end().map(move || Meta {
        title: Cow::Borrowed("Skills"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: None,
    });
    let skill_get = base.and(warp::path::param::<i32>()).map(skill_get_impl);
    let skills = warp::path("skills").and(skills_end.or(skill_get).unify());

    let catch = warp::any().map(move || Meta {
        title: Cow::Borrowed("LU-Explorer"),
        description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
        image: None,
    });
    objects
        .or(missions)
        .unify()
        .or(dashboard)
        .unify()
        .or(skills)
        .unify()
        .or(catch)
        .unify()
}

#[allow(clippy::needless_lifetimes)] // false positive?
pub(crate) fn make_spa_dynamic<'r>(
    data: &'static TypedDatabase<'static>,
    hb: Arc<Handlebars<'r>>,
    domain: &str,
    //    hnd: ArcHandle<B, FDBHeader>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = Infallible> + Clone + 'r {
    let dom = {
        let d = Box::leak(domain.to_string().into_boxed_str()) as &str;
        warp::any().map(move || d)
    };

    // Prepare the default image
    let mut default_img = data.lu_res_prefix.to_owned();
    default_img.push_str(DEFAULT_IMG);
    let default_img: &'static str = Box::leak(default_img.into_boxed_str());

    // Create a reusable closure to render template
    let handlebars = move |with_template| render(with_template, hb.clone());

    warp::any()
        .and(dom)
        .and(meta(data))
        .and(warp::path::full())
        .map(move |dom: &str, meta: Meta, full_path: FullPath| WithTemplate {
            name: "template.html",
            value: IndexParams {
                title: meta.title,
                r#type: "website",
                card: "summary",
                description: meta.description,
                site: Cow::Borrowed("@lu_explorer"),
                image: meta
                    .image
                    .map(Cow::Owned)
                    .unwrap_or(Cow::Borrowed(default_img)),
                url: Cow::Owned(format!("https://{}{}", dom, full_path.as_str())),
            },
        })
        .map(handlebars)
}
