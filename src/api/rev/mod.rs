//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
pub(crate) use self::routes::Route;
use super::adapter::Keys;
use crate::data::locale::LocaleRoot;
pub use data::ReverseLookup;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::{
    io, str,
    task::{Context, Poll},
};
use tower::Service;

mod behaviors;
mod common;
mod component_types;
mod data;
mod loot_table_index;
mod missions;
mod object_types;
mod routes;
mod skills;

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
            Route::Activities => super::reply(a, &Keys::new(&self.rev.activities)),
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
            Route::ObjectTypes => super::reply(a, &Keys::new(&self.rev.object_types)),
            Route::ObjectTypeByName(ty) => {
                super::reply(a, &object_types::rev_object_type(self.db, self.rev, ty))
            }
            Route::SkillById(skill_id) => {
                super::reply(a, &skills::rev_skill_id(self.db, self.rev, skill_id))
            }
            Route::GateVersions => super::reply(a, &self.rev.gate_versions.keys()),
            Route::GateVersionByName(name) => {
                super::reply_opt(a, self.rev.gate_versions.get(&name.0))
            }
            Route::Objects => super::reply(a, &Keys::new(&self.rev.objects.rev)),
            Route::ObjectById(id) => super::reply_opt(a, self.rev.objects.rev.get(&id)),
        };
        std::future::ready(r)
    }
}
