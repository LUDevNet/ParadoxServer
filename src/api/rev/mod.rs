//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
use super::PercentDecoded;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::{
    convert::Infallible,
    io, str,
    task::{Context, Poll},
};
use tower::Service;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

mod common;
mod data;

mod behaviors;
mod component_types;
mod loot_table_index;
mod missions;
mod object_types;
mod skills;

pub use data::ReverseLookup;

use crate::data::locale::LocaleRoot;

use super::{adapter::BTreeMapKeysAdapter, tydb_filter};

#[derive(Debug, Clone, Serialize)]
pub struct Api<T, E> {
    #[serde(flatten)]
    data: T,
    #[serde(rename = "_embedded")]
    embedded: E,
}

static REV_APIS: &[&str; 8] = &[
    "activity",
    "behaviors",
    "component_types",
    "loot_table_index",
    "mission_types",
    "objects",
    "object_types",
    "skill_ids",
];

#[derive(Debug, Copy, Clone)]
pub(crate) struct Rev<'a> {
    inner: &'a ReverseLookup,
}

fn rev_filter<'a>(
    inner: &'a ReverseLookup,
) -> impl Filter<Extract = (Rev,), Error = Infallible> + Clone + 'a {
    warp::any().map(move || Rev { inner })
}

type Ext = (&'static TypedDatabase<'static>, Rev<'static>);

pub(super) fn make_api_rev(
    db: &'static TypedDatabase<'static>,
    rev: &'static ReverseLookup,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let db = tydb_filter(db);
    let rev = db.and(rev_filter(rev));

    let rev_object_types = object_types::object_types_api(&rev);
    let rev_skills = skills::skill_api(&rev);

    rev_skills.or(rev_object_types).unify().boxed()
}

#[derive(Debug)]
pub(super) enum Route {
    Base,
    Activities,
    ActivityById(i32),
    BehaviorById(i32),
    ComponentTypes,
    ComponentTypeById(i32),
    ComponentTypeByIdAndCid(i32, i32),
    LootTableIndexById(i32),
    MissionTypes,
    MissionTypesFull,
    MissionTypeByTy(PercentDecoded),
    MissionTypeBySubTy(PercentDecoded, PercentDecoded),
    ObjectsSearchIndex,
}

impl Route {
    fn lti_from_parts(mut parts: str::Split<'_, char>) -> Result<Self, ()> {
        match parts.next() {
            Some(key) => match key.parse() {
                Ok(id) => match parts.next() {
                    None => Ok(Self::LootTableIndexById(id)),
                    Some("") => match parts.next() {
                        None => Ok(Self::LootTableIndexById(id)),
                        Some(_) => Err(()),
                    },
                    _ => Err(()),
                },
                Err(_) => Err(()),
            },
            _ => Err(()),
        }
    }

    pub(super) fn from_parts(mut parts: str::Split<'_, char>) -> Result<Self, ()> {
        match parts.next() {
            Some("activity" | "activities") => match parts.next() {
                Some("") => match parts.next() {
                    None => Ok(Self::Activities),
                    _ => Err(()),
                },
                Some(key) => match parts.next() {
                    None => match key.parse() {
                        Ok(id) => Ok(Self::ActivityById(id)),
                        Err(_) => Err(()),
                    },
                    _ => Err(()),
                },
                None => Ok(Self::Activities),
            },
            Some("behaviors") => match parts.next() {
                Some(key) => match key.parse() {
                    Ok(id) => Ok(Self::BehaviorById(id)),
                    Err(_) => Err(()),
                },
                _ => Err(()),
            },
            Some("component_types" | "component-types") => match parts.next() {
                Some("") => match parts.next() {
                    None => Ok(Self::ComponentTypes),
                    _ => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(id) => match parts.next() {
                        None => Ok(Self::ComponentTypeById(id)),
                        Some("") => match parts.next() {
                            Some(_) => Err(()),
                            None => Ok(Self::ComponentTypeById(id)),
                        },
                        Some(key2) => match key2.parse() {
                            Ok(cid) => match parts.next() {
                                None => Ok(Self::ComponentTypeByIdAndCid(id, cid)),
                                Some("") => match parts.next() {
                                    Some(_) => Err(()),
                                    None => Ok(Self::ComponentTypeByIdAndCid(id, cid)),
                                },
                                Some(_) => Err(()),
                            },
                            Err(_) => Err(()),
                        },
                    },
                    Err(_) => Err(()),
                },
                None => Ok(Self::ComponentTypes),
            },
            Some("loot_table_index") => Self::lti_from_parts(parts),
            Some("loot-tables") => match parts.next() {
                Some("indices") => Self::lti_from_parts(parts),
                Some(_) => Err(()),
                None => Err(()),
            },
            Some("mission-types") => match parts.next() {
                None => Ok(Self::MissionTypes),
                Some("") => match parts.next() {
                    None => Ok(Self::MissionTypes),
                    Some(_) => Err(()),
                },
                Some("full") => match parts.next() {
                    None => Ok(Self::MissionTypesFull),
                    Some("") => match parts.next() {
                        None => Ok(Self::MissionTypesFull),
                        Some(_) => Err(()),
                    },
                    Some(_) => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(d_type) => match parts.next() {
                        None => Ok(Self::MissionTypeByTy(d_type)),
                        Some("") => match parts.next() {
                            None => Ok(Self::MissionTypeByTy(d_type)),
                            Some(_) => Err(()),
                        },
                        Some(key2) => match key2.parse() {
                            Ok(d_subtype) => match parts.next() {
                                None => Ok(Self::MissionTypeBySubTy(d_type, d_subtype)),
                                Some("") => match parts.next() {
                                    None => Ok(Self::MissionTypeBySubTy(d_type, d_subtype)),
                                    Some(_) => Err(()),
                                },
                                Some(_) => Err(()),
                            },
                            Err(_) => Err(()),
                        },
                    },
                    Err(_) => Err(()),
                },
            },
            Some("objects") => match parts.next() {
                Some("search_index" | "search-index") => match parts.next() {
                    None => Ok(Self::ObjectsSearchIndex),
                    Some("") => match parts.next() {
                        None => Ok(Self::ObjectsSearchIndex),
                        _ => Err(()),
                    },
                    Some(_) => Err(()),
                },
                _ => Err(()),
            },
            Some("") => match parts.next() {
                None => Ok(Self::Base),
                _ => Err(()),
            },
            None => Ok(Self::Base),
            _ => Err(()),
        }
    }
}

#[derive(Clone)]
pub struct RevService {
    db: &'static TypedDatabase<'static>,
    loc: LocaleRoot,
    rev: &'static ReverseLookup,
}

impl RevService {
    pub(crate) fn new(
        db: &'static TypedDatabase<'static>,
        loc: LocaleRoot,
        rev: &'static ReverseLookup,
    ) -> RevService {
        Self { db, loc, rev }
    }
}

impl Service<(super::Accept, Route)> for RevService {
    type Response = http::Response<hyper::Body>;
    type Error = io::Error;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (a, route): (super::Accept, Route)) -> Self::Future {
        let r = match route {
            Route::Base => super::reply_json(&REV_APIS),
            Route::Activities => super::reply(a, &BTreeMapKeysAdapter::new(&self.rev.activities)),
            Route::ActivityById(id) => super::reply_opt(a, self.rev.activities.get(&id)),
            Route::BehaviorById(id) => super::reply(a, &behaviors::lookup(self.db, self.rev, id)),
            Route::ComponentTypes => super::reply(a, &component_types::Components::new(self.rev)),
            Route::ComponentTypeById(id) => super::reply(
                a,
                &component_types::rev_component_type(self.db, self.rev, id),
            ),
            Route::ComponentTypeByIdAndCid(key, cid) => super::reply(
                a,
                &component_types::rev_single_component(self.rev, key, cid),
            ),
            Route::LootTableIndexById(id) => super::reply(
                a,
                &loot_table_index::rev_loop_table_index(self.db, self.rev, id),
            ),
            Route::MissionTypes => super::reply(a, &missions::MissionTypesAdapter::new(self.rev)),
            Route::MissionTypesFull => super::reply(a, &self.rev.mission_types),
            Route::MissionTypeByTy(ty) => super::reply(
                a,
                &missions::rev_mission_type(self.db, self.rev, &self.loc, ty),
            ),
            Route::MissionTypeBySubTy(d_type, d_subtype) => super::reply(
                a,
                &missions::rev_mission_subtype(self.db, self.rev, &self.loc, d_type, d_subtype),
            ),
            Route::ObjectsSearchIndex => super::reply(a, &self.rev.objects.search_index),
        };
        std::future::ready(r)
    }
}
