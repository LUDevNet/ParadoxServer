use std::convert::Infallible;

use assembly_core::buffer::CastError;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use super::{
    common::{ObjectTypeEmbedded, ObjectsRefAdapter},
    data::{ComponentsUse, ReverseLookup},
    Api, Ext, Rev,
};
use crate::api::{adapter::BTreeMapKeysAdapter, map_opt_res};

#[derive(Serialize)]
pub(super) struct Components<'a> {
    components: BTreeMapKeysAdapter<'a, i32, ComponentsUse>,
}

impl<'a> Components<'a> {
    pub fn new(rev: &'a ReverseLookup) -> Self {
        Self {
            components: BTreeMapKeysAdapter::new(&rev.component_use),
        }
    }
}

fn rev_component_type_api(
    _db: &TypedDatabase,
    rev: Rev,
    key: i32,
) -> Result<Option<Json>, CastError> {
    let val = rev.inner.component_use.get(&key);
    Ok(val.map(|data| {
        let keys: Vec<i32> = data
            .components
            .iter()
            .flat_map(|(_, u)| u.lots.iter().copied())
            .collect();
        let embedded = ObjectTypeEmbedded {
            objects: ObjectsRefAdapter::new(&_db.objects, &keys),
        };
        warp::reply::json(&Api { data, embedded })
    }))
}

fn rev_single_component_api(
    _db: &TypedDatabase,
    rev: Rev,
    key: i32,
    cid: i32,
) -> Result<Option<Json>, CastError> {
    let val = rev
        .inner
        .component_use
        .get(&key)
        .and_then(|c| c.components.get(&cid));
    Ok(val.map(warp::reply::json))
}

pub(super) fn component_types_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_component_types_base = rev.clone().and(warp::path("component_types"));

    let rev_single_component_type = rev_component_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_single_component_api)
        .map(map_opt_res)
        .boxed();

    let rev_component_type = rev_component_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_component_type_api)
        .map(map_opt_res)
        .boxed();

    rev_single_component_type
        .or(rev_component_type)
        .unify()
        .boxed()
}
