use super::{common::MissionTasks, Api, Ext, Rev};
use crate::api::map_res;
use assembly_core::buffer::CastError;
use paradox_typed_db::TypedDatabase;
use serde::Serialize;
use std::convert::Infallible;
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

#[derive(Clone, Serialize)]
pub struct SkillIDEmbedded<'a, 'b> {
    #[serde(rename = "MissionTasks")]
    mission_tasks: MissionTasks<'a, 'b>,
    //MapFilter<'a, MissionTaskUIDLookup>,
}

fn rev_skill_id_api(db: &'_ TypedDatabase, rev: Rev, skill_id: i32) -> Result<Json, CastError> {
    let h = rev.inner.skill_ids.get(&skill_id).map(|data| {
        let mission_tasks = MissionTasks {
            index: &rev.inner.mission_task_uids,
            keys: &data.mission_tasks[..],
            table: &db.mission_tasks,
            id_col: db.mission_tasks.col_uid,
        };
        Api {
            data,
            embedded: SkillIDEmbedded { mission_tasks },
        }
    });
    Ok(warp::reply::json(&h))
}

pub(super) fn skill_api<
    F: Filter<Extract = Ext, Error = Infallible> + Send + Sync + Clone + 'static,
>(
    rev: &F,
) -> BoxedFilter<(WithStatus<Json>,)> {
    let rev_skill_ids = rev.clone().and(warp::path("skill_ids"));
    let rev_skill_id_base = rev_skill_ids.and(warp::path::param::<i32>());
    let rev_skill_id = rev_skill_id_base
        .and(warp::path::end())
        .map(rev_skill_id_api)
        .map(map_res);
    rev_skill_id.boxed()
}
