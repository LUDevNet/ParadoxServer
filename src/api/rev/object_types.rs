use std::{borrow::Borrow, convert::Infallible};

use assembly_core::buffer::CastError;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use super::{common::ObjectTypeEmbedded, Ext};
use crate::api::{
    adapter::TypedTableIterAdapter,
    map_opt_res, map_res,
    rev::{Api, Rev},
    PercentDecoded,
};

#[derive(Serialize)]
struct ObjectIDs<'a, T> {
    object_ids: &'a [T],
}

fn rev_object_type_api(
    db: &TypedDatabase,
    rev: Rev,
    ty: PercentDecoded,
) -> Result<Option<Json>, CastError> {
    let key: &String = ty.borrow();
    tracing::info!("{}", key);
    Ok(rev.inner.object_types.get(key).map(|objects| {
        let rep = Api {
            data: ObjectIDs {
                object_ids: objects.as_ref(),
            },
            embedded: ObjectTypeEmbedded {
                objects: TypedTableIterAdapter::new(&db.objects, objects),
            },
        };
        warp::reply::json(&rep)
    }))
}

fn rev_object_types_api(_db: &TypedDatabase, rev: Rev) -> Result<Json, CastError> {
    let keys: Vec<_> = rev.inner.object_types.keys().collect();
    Ok(warp::reply::json(&keys))
}

pub(super) fn object_types_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_object_types_base = rev.clone().and(warp::path("object_types"));

    let rev_object_type = rev_object_types_base
        .clone()
        .and(warp::path::param())
        .and(warp::path::end())
        .map(rev_object_type_api)
        .map(map_opt_res)
        .boxed();

    let rev_object_types_list = rev_object_types_base
        .clone()
        .and(warp::path::end())
        .map(rev_object_types_api)
        .map(map_res)
        .boxed();

    rev_object_type.or(rev_object_types_list).unify().boxed()
}
