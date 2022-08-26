//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
use super::PercentDecoded;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::{
    io, str,
    task::{Context, Poll},
};
use tower::Service;

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

use super::adapter::BTreeMapKeysAdapter;

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
    ObjectTypes,
    ObjectTypeByName(PercentDecoded),
    SkillById(i32),
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
            Some("mission_types" | "mission-types") => Self::mission_types_from_parts(parts),
            Some("missions") => match parts.next() {
                Some("types") => Self::mission_types_from_parts(parts),
                _ => Err(()),
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
            Some("object_types") => match parts.next() {
                None => Ok(Self::ObjectTypes),
                Some("") => match parts.next() {
                    None => Ok(Self::ObjectTypes),
                    _ => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(ty) => match parts.next() {
                        None => Ok(Self::ObjectTypeByName(ty)),
                        Some("") => match parts.next() {
                            None => Ok(Self::ObjectTypeByName(ty)),
                            _ => Err(()),
                        },
                        Some(_) => Err(()),
                    },
                    Err(_) => Err(()),
                },
            },
            Some("skill_ids" | "skills") => match parts.next() {
                Some(key) => match key.parse() {
                    Ok(id) => match parts.next() {
                        None => Ok(Self::SkillById(id)),
                        Some("") => match parts.next() {
                            None => Ok(Self::SkillById(id)),
                            Some(_) => Err(()),
                        },
                        Some(_) => Err(()),
                    },
                    Err(_) => Err(()),
                },
                None => Err(()),
            },
            Some("") => match parts.next() {
                None => Ok(Self::Base),
                _ => Err(()),
            },
            None => Ok(Self::Base),
            _ => Err(()),
        }
    }

    fn mission_types_from_parts(mut parts: str::Split<char>) -> Result<Route, ()> {
        match parts.next() {
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
            Route::ObjectTypes => {
                super::reply(a, &BTreeMapKeysAdapter::new(&self.rev.object_types))
            }
            Route::ObjectTypeByName(ty) => {
                super::reply(a, &object_types::rev_object_type(self.db, self.rev, ty))
            }
            Route::SkillById(skill_id) => {
                super::reply(a, &skills::rev_skill_id(self.db, self.rev, skill_id))
            }
        };
        std::future::ready(r)
    }
}
