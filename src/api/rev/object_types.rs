use std::borrow::Borrow;

use paradox_typed_db::TypedDatabase;
use serde::Serialize;

use super::{common::ObjectTypeEmbedded, ReverseLookup};
use crate::api::{
    rev::{common::ObjectsRefAdapter, Api},
    PercentDecoded,
};

#[derive(Serialize)]
pub(super) struct ObjectIDs<'a, T> {
    object_ids: &'a [T],
}

pub(super) fn rev_object_type<'a, 'b, 'r>(
    db: &'b TypedDatabase<'a>,
    rev: &'r ReverseLookup,
    ty: PercentDecoded,
) -> Option<Api<ObjectIDs<'r, i32>, ObjectTypeEmbedded<'a, 'b, &'r [i32]>>> {
    let key: &String = ty.borrow();
    let object_ids: &[i32] = rev.object_types.get(key)?.as_ref();
    Some(Api {
        data: ObjectIDs { object_ids },
        embedded: ObjectTypeEmbedded {
            objects: ObjectsRefAdapter::new(&db.objects, object_ids),
        },
    })
}
