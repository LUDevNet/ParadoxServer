//! # Reverse API `/rev`
//!
//! This module contains the reverse API of the server. These are, generally speaking,
//! database lookups by some specific ID such as an "object template id" or a "skill id"
//! and produce data from multiple tables.
pub(crate) use self::routes::Route;
use self::{factions::FactionById, routes::REV_APIS};
use super::adapter::Keys;
use crate::data::locale::LocaleRoot;
pub use data::ReverseLookup;
use http::{Method, StatusCode};
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::task::{Context, Poll};
use tower::Service;

mod behaviors;
mod common;
mod component_types;
mod data;
mod factions;
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

impl Service<(super::Accept, Method, Route)> for RevService {
    type Response = http::Response<hyper::Body>;
    type Error = super::ApiError;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (a, method, route): (super::Accept, Method, Route)) -> Self::Future {
        if method != Method::GET && method != Method::HEAD {
            // For now, only allow GET requests
            return std::future::ready(Ok(super::reply_405(&super::ALLOW_GET_HEAD)));
        }
        if method == Method::HEAD {
            return std::future::ready(Ok(super::reply_200(a)));
        }
        let r = match route {
            Route::Base => super::reply_json(&REV_APIS, StatusCode::OK),
            Route::Activities => super::reply(a, &Keys::new(&self.rev.activities), StatusCode::OK),
            Route::ActivityById(id) => super::reply_opt(a, self.rev.activities.get(&id)),
            Route::BehaviorById(id) => {
                super::reply(a, &behaviors::lookup(self.db, self.rev, id), StatusCode::OK)
            }
            Route::ComponentTypes => super::reply(
                a,
                &component_types::Components::new(self.rev),
                StatusCode::OK,
            ),
            Route::ComponentTypeById(id) => super::reply(
                a,
                &component_types::rev_component_type(self.db, self.rev, id),
                StatusCode::OK,
            ),
            Route::ComponentTypeByIdAndCid(key, cid) => super::reply(
                a,
                &component_types::rev_single_component(self.rev, key, cid),
                StatusCode::OK,
            ),
            Route::Factions => super::reply(a, &Keys::new(&self.rev.factions), StatusCode::OK),
            Route::FactionById(id) => {
                super::reply(a, &FactionById::new(self.rev, id), StatusCode::OK)
            }
            Route::LootTableIndexById(id) => super::reply(
                a,
                &loot_table_index::rev_loop_table_index(self.db, self.rev, id),
                StatusCode::OK,
            ),
            Route::Missions => super::reply(a, &Keys::new(&self.rev.missions), StatusCode::OK),
            Route::MissionById(id) => {
                super::reply_opt(a, missions::mission_by_id(self.rev, id).as_ref())
            }
            Route::MissionTypes => super::reply(
                a,
                &missions::MissionTypesAdapter::new(self.rev),
                StatusCode::OK,
            ),
            Route::MissionTypesFull => super::reply(a, &self.rev.mission_types, StatusCode::OK),
            Route::MissionTypeByTy(ty) => super::reply(
                a,
                &missions::rev_mission_type(self.db, self.rev, &self.loc, ty),
                StatusCode::OK,
            ),
            Route::MissionTypeBySubTy(d_type, d_subtype) => super::reply(
                a,
                &missions::rev_mission_subtype(self.db, self.rev, &self.loc, d_type, d_subtype),
                StatusCode::OK,
            ),
            Route::ObjectsSearchIndex => {
                super::reply(a, &self.rev.objects.search_index, StatusCode::OK)
            }
            Route::ObjectTypes => {
                super::reply(a, &Keys::new(&self.rev.object_types), StatusCode::OK)
            }
            Route::ObjectTypeByName(ty) => super::reply(
                a,
                &object_types::rev_object_type(self.db, self.rev, ty),
                StatusCode::OK,
            ),
            Route::SkillById(skill_id) => super::reply(
                a,
                &skills::rev_skill_id(self.db, self.rev, skill_id),
                StatusCode::OK,
            ),
            Route::SkillCooldownGroups => super::reply(
                a,
                &Keys::new(&self.rev.skill_cooldown_groups),
                StatusCode::OK,
            ),
            Route::SkillCooldownGroupById(id) => {
                super::reply_opt(a, self.rev.skill_cooldown_groups.get(&id))
            }
            Route::GateVersions => super::reply(a, &self.rev.gate_versions.keys(), StatusCode::OK),
            Route::GateVersionByName(name) => {
                super::reply_opt(a, self.rev.gate_versions.get(&name.0))
            }
            Route::Objects => super::reply(a, &Keys::new(&self.rev.objects.rev), StatusCode::OK),
            Route::ObjectById(id) => super::reply_opt(a, self.rev.objects.rev.get(&id)),
        };
        std::future::ready(r)
    }
}
