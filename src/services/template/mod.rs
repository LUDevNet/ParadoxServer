use color_eyre::eyre::Context;
use http::{uri::PathAndQuery, Response};
use notify::{
    event::{AccessKind, AccessMode, EventKind, RemoveKind},
    recommended_watcher, RecursiveMode, Watcher,
};
use pin_project::pin_project;
use std::{
    borrow::Cow,
    ffi::OsStr,
    fmt::{self, Write},
    io,
    path::Path,
    pin::Pin,
    sync::{Arc, RwLock},
    task::Poll,
};
use tower_service::Service;

use paradox_typed_db::{ext::MissionKind, TypedDatabase};
use regex::{Captures, Regex};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info};

mod minihb;
pub(crate) use minihb::Template;

use crate::data::{
    fs::{cleanup_path, LuRes},
    locale::LocaleRoot,
};

fn make_meta_template(text: &str) -> Cow<str> {
    let re = Regex::new("<meta\\s+(name|property)=\"(.*?)\"\\s+content=\"(.*)\"\\s*/?>").unwrap();
    re.replace_all(text, |cap: &Captures| {
        let kind = &cap[1];
        let name = &cap[2];
        let value = match name {
            "twitter:title" | "og:title" => "{{title}}",
            "twitter:description" | "og:description" => "{{description}}",
            "twitter:image" | "og:image" => "{{image}}",
            "og:url" => "{{url}}",
            "og:type" => "{{type}}",
            "twitter:card" => "{{card}}",
            "twitter:site" => "{{site}}",
            _ => &cap[3],
        };
        format!("<meta {}=\"{}\" content=\"{}\">", kind, name, value)
    })
}

pub struct FsEventHandler {
    tx: Sender<notify::Result<notify::Event>>,
}

impl FsEventHandler {
    pub fn new(tx: Sender<notify::Result<notify::Event>>) -> Self {
        Self { tx }
    }
}

impl notify::EventHandler for FsEventHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        self.tx.blocking_send(event).unwrap();
    }
}

/// This is a future that completes when the incoming stream completes
#[pin_project]
pub struct TemplateUpdateTask {
    rx: Receiver<notify::Result<notify::Event>>,
    hb: Arc<RwLock<Template>>,
}

impl TemplateUpdateTask {
    pub(crate) fn new(
        rx: Receiver<notify::Result<notify::Event>>,
        hb: Arc<RwLock<Template>>,
    ) -> Self {
        Self { rx, hb }
    }
}

impl std::future::Future for TemplateUpdateTask {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        while let Poll::Ready(r) = this.rx.poll_recv(cx) {
            let e = match r {
                Some(Ok(e)) => e,
                Some(Err(e)) => {
                    tracing::error!("filesystem watch failure: {}", e);
                    continue;
                }
                None => return Poll::Ready(()),
            };

            debug!("filesystem watch event: {:?}", e);
            if !matches!(
                e.kind,
                EventKind::Access(AccessKind::Close(AccessMode::Write))
                    | EventKind::Remove(RemoveKind::File)
            ) {
                continue;
            }
            for p in e.paths {
                debug!("Updated file: {}", p.display());
                if p.file_name() != Some(OsStr::new("index.html")) {
                    continue;
                }
                if let Err(e) = load_meta_template(this.hb, &p) {
                    error!("Failed to re-load template: {}", e);
                }
            }
        }
        Poll::Pending
    }
}

pub(crate) fn load_meta_template(
    reg: &RwLock<Template>,
    path: &Path,
) -> Result<(), color_eyre::Report> {
    info!("(re-)loading template.html");
    let index_text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to load '{}'", path.display()))?;
    let tpl_str = make_meta_template(&index_text);
    let mut hb = reg
        .write()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to acquire handlebars lock: {}", e))?;
    hb.set_text(tpl_str);
    Ok(())
}

pub(crate) fn spawn_watcher(path: &Path, hb: Arc<RwLock<Template>>) -> Result<(), notify::Error> {
    // Setup the watcher
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let eh = FsEventHandler::new(tx);
    let mut watcher = recommended_watcher(eh)?;
    watcher.watch(path, RecursiveMode::Recursive)?;

    let rt = tokio::runtime::Handle::current();
    rt.spawn(TemplateUpdateTask::new(rx, hb));
    Ok(())
}

/// Retrieve metadata for /missions/:id
fn mission_get_impl(data: &'_ TypedDatabase<'_>, loc: LocaleRoot, res: LuRes, id: i32) -> Meta {
    let mut image = None;
    let mut kind = MissionKind::Mission;
    if let Some(mission) = data.get_mission_data(id) {
        if !mission.is_mission {
            kind = MissionKind::Achievement;
            if let Some(icon_id) = mission.mission_icon_id {
                if let Some(path) = data.get_icon_path(icon_id) {
                    image = cleanup_path(path).map(|p| res.to_res_href(&p));
                }
            }
        }
    }

    let mut desc = String::new();

    let tasks = data.get_mission_tasks(id);
    let tasks_locale = loc.root.str_children.get("MissionTasks").unwrap();
    for task in tasks {
        if image.is_none() {
            if let Some(icon_id) = task.icon_id {
                if let Some(path) = data.get_icon_path(icon_id) {
                    image = cleanup_path(path).map(|p| res.to_res_href(&p));
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

    let title = loc
        .get_mission_name(kind, id)
        .unwrap_or(format!("Missing {:?} #{}", kind, id));

    Meta {
        title: Cow::Owned(title),
        description: Cow::Owned(desc),
        image,
    }
}

/// Retrieve metadata for /objects/:id
fn object_get_api(data: &'_ TypedDatabase<'_>, _loc: LocaleRoot, res: LuRes, id: i32) -> Meta {
    let (title, description) = data
        .get_object_name_desc(id)
        .unwrap_or((format!("Missing Object #{}", id), String::new()));
    let comp = data.get_components(id);
    let image = comp.render.and_then(|id| data.get_render_image(id));
    let image = image.and_then(cleanup_path).map(|p| res.to_res_href(&p));
    Meta {
        title: Cow::Owned(title),
        description: Cow::Owned(description),
        image,
    }
}

/// Retrieve metadata for /objects/item-sets/:id
fn item_set_get_impl(data: &'_ TypedDatabase<'_>, loc: LocaleRoot, res: LuRes, id: i32) -> Meta {
    let mut rank = 0;
    let mut image = None;
    let mut desc = String::new();
    if let Some(item_set) = data.item_sets.get_data(id) {
        rank = item_set.kit_rank;
        if let Some(image_id) = item_set.kit_image {
            if let Some(path) = data.get_icon_path(image_id).and_then(cleanup_path) {
                image = Some(res.to_res_href(&path));
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

    let title = loc
        .get_item_set_name(rank, id)
        .unwrap_or(format!("Unnamed Item Set #{}", id));

    Meta {
        title: Cow::Owned(title),
        description: Cow::Owned(desc),
        image,
    }
}

/// Retrieve metadata for /skills/:id
fn skill_get_impl(data: &'_ TypedDatabase<'_>, loc: &LocaleRoot, res: &LuRes, id: i32) -> Meta {
    let (mut title, description) = loc.get_skill_name_desc(id);
    let description = description.map(Cow::Owned).unwrap_or(Cow::Borrowed(""));
    let mut image = None;

    if let Some(skill) = data.skills.get_data(id) {
        if title.is_none() {
            title = Some(format!("Skill #{}", id))
        }
        if let Some(icon_id) = skill.skill_icon {
            if let Some(path) = data.get_icon_path(icon_id).and_then(cleanup_path) {
                image = Some(res.to_res_href(&path));
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

#[derive(Debug, Copy, Clone)]
pub enum SpaRoute {
    Dashboard,
    Objects,
    ObjectById { id: i32 },
    Missions,
    MissionById(i32),
    Skills,
    SkillById { id: i32 },
    ItemSets,
    ItemSetById { id: i32 },
}

impl SpaRoute {
    fn parse(path: &str) -> Option<Self> {
        let mut split = path.trim_start_matches('/').split('/');
        match split.next() {
            Some("dashboard") => Some(Self::Dashboard),
            Some("objects") => match split.next() {
                Some("item-sets") => match split.next() {
                    Some(x) => match x.parse::<i32>() {
                        Ok(id) => Some(Self::ItemSetById { id }),
                        Err(_) => None,
                    },
                    _ => Some(Self::ItemSets),
                },
                Some(x) => match x.parse::<i32>() {
                    Ok(id) => Some(Self::ObjectById { id }),
                    Err(_) => None,
                },
                _ => Some(Self::Objects),
            },
            Some("missions") => match split.next() {
                Some(x) => x.parse::<i32>().ok().map(Self::MissionById),
                _ => Some(Self::Missions),
            },
            Some("skills") => match split.next() {
                Some(x) => match x.parse::<i32>() {
                    Ok(id) => Some(Self::SkillById { id }),
                    Err(_) => None,
                },
                _ => Some(Self::Skills),
            },
            _ => None,
        }
    }

    fn to_meta(self, data: &'_ TypedDatabase<'_>, loc: &LocaleRoot, res: &LuRes) -> Meta {
        match self {
            Self::Dashboard => Meta::DASHBOARD,
            Self::Objects => Meta::OBJECTS,
            Self::ObjectById { id } => object_get_api(data, loc.clone(), res.clone(), id),
            Self::Missions => Meta::MISSIONS,
            Self::MissionById(id) => mission_get_impl(data, loc.clone(), res.clone(), id),
            Self::Skills => Meta::SKILLS,
            Self::SkillById { id } => skill_get_impl(data, loc, res, id),
            Self::ItemSets => Meta::ITEM_SETS,
            Self::ItemSetById { id } => item_set_get_impl(data, loc.clone(), res.clone(), id),
        }
    }
}

pub struct IndexParams {
    pub title: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub r#type: &'static str,
    pub image: Cow<'static, str>,
    pub url: Cow<'static, str>,
    pub card: &'static str,
    pub site: Cow<'static, str>,
}

impl minihb::Lookup for IndexParams {
    fn field(&self, key: &str) -> &dyn std::fmt::Display {
        match key {
            "title" => &self.title,
            "description" => &self.description,
            "type" => &self.r#type,
            "image" => &self.image,
            "url" => &self.url,
            "card" => &self.card,
            "site" => &self.site,
            _ => &"",
        }
    }
}

static DEFAULT_IMG: &str = "/ui/ingame/freetrialcongratulations_id.png";

#[derive(Debug, Clone)]
struct Meta {
    title: Cow<'static, str>,
    description: Cow<'static, str>,
    image: Option<String>,
}

impl Meta {
    const DASHBOARD: Self = Self {
        title: Cow::Borrowed("Dashboard"),
        description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
        image: None,
    };

    const OBJECTS: Self = Self {
        title: Cow::Borrowed("Objects"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: None,
    };

    const ITEM_SETS: Self = Self {
        title: Cow::Borrowed("Item Sets"),
        description: Cow::Borrowed("Check out the LEGO Universe Objects"),
        image: None,
    };

    const MISSIONS: Self = Self {
        title: Cow::Borrowed("Missions"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: None,
    };

    const SKILLS: Self = Self {
        title: Cow::Borrowed("Skills"),
        description: Cow::Borrowed("Check out the LEGO Universe Missions"),
        image: None,
    };
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            title: Cow::Borrowed("LU-Explorer"),
            description: Cow::Borrowed("Check out the LEGO Universe Game Data"),
            image: None,
        }
    }
}

#[derive(Clone)]
struct RenderService {
    template: Arc<RwLock<Template>>,
}

#[derive(Debug)]
struct LockError;

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LockError")
    }
}

impl std::error::Error for LockError {}

impl tower_service::Service<IndexParams> for RenderService {
    type Response = String;
    type Error = LockError;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: IndexParams) -> Self::Future {
        std::future::ready(
            self.template
                .read()
                .map(|r| r.render(&req))
                .map_err(|_e| LockError),
        )
    }
}

#[derive(Clone)]
pub struct SpaDynamic {
    inner: RenderService,
    data: &'static TypedDatabase<'static>,
    default_img: &'static str,
    locale_root: LocaleRoot,
    res: LuRes,
    base_url: &'static str,
}

impl SpaDynamic {
    pub fn new(
        data: &'static TypedDatabase<'static>,
        locale_root: LocaleRoot,
        res: LuRes,
        hb: Arc<RwLock<Template>>,
        base_url: &str,
    ) -> Self {
        let base_url = Box::leak(base_url.to_string().into_boxed_str()) as &str;

        // Prepare the default image
        let default_img = res.to_res_href(Path::new(DEFAULT_IMG));
        let default_img: &'static str = Box::leak(default_img.into_boxed_str());

        // Create a reusable closure to render template
        let inner = RenderService { template: hb };
        Self {
            inner,
            data,
            locale_root,
            res,
            default_img,
            base_url,
        }
    }

    fn meta<ReqBody>(&self, req: &http::Request<ReqBody>) -> Meta {
        let path = req.uri().path();
        if let Some(route) = SpaRoute::parse(path) {
            route.to_meta(self.data, &self.locale_root, &self.res)
        } else {
            Meta::default()
        }
    }
}

#[pin_project]
pub struct SpaFuture {
    #[pin]
    inner: std::future::Ready<Result<String, LockError>>,
}

impl std::future::Future for SpaFuture {
    type Output = Result<Response<hyper::Body>, io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(|r| match r {
            Ok(s) => Ok(Response::new(hyper::Body::from(s))),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        })
    }
}

impl<ReqBody> Service<http::Request<ReqBody>> for SpaDynamic
where
    ReqBody: Send + 'static,
{
    type Response = Response<hyper::Body>;
    type Error = io::Error;
    type Future = SpaFuture;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        let meta = self.meta(&req);
        let full_path = req
            .uri()
            .path_and_query()
            .map(PathAndQuery::as_str)
            .unwrap_or_default();
        let params = IndexParams {
            title: meta.title,
            r#type: "website",
            card: "summary",
            description: meta.description,
            site: Cow::Borrowed("@lu_explorer"),
            image: meta
                .image
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed(self.default_img)),
            url: Cow::Owned(self.base_url.to_string() + full_path),
        };
        SpaFuture {
            inner: self.inner.call(params),
        }
    }
}
