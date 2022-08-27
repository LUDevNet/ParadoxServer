use super::{common::MissionTasks, data::SkillIdLookup, Api, ReverseLookup};
use crate::api::adapter::{AdapterLayout, I32Slice};
use paradox_typed_db::{columns::MissionTasksColumn, TypedDatabase};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(super) struct SkillIDEmbedded<'a, 'b> {
    #[serde(rename = "MissionTasks")]
    mission_tasks: MissionTasks<'a, 'b>,
    //MapFilter<'a, MissionTaskUIDLookup>,
}

type SkillApiResult<'a, 'b> = Api<&'b SkillIdLookup, SkillIDEmbedded<'a, 'b>>;

pub(super) fn rev_skill_id<'a, 'b>(
    db: &'b TypedDatabase<'a>,
    rev: &'b ReverseLookup,
    skill_id: i32,
) -> Option<SkillApiResult<'a, 'b>> {
    let data = rev.skill_ids.get(&skill_id)?;
    let mission_tasks = MissionTasks {
        index: &rev.mission_task_uids,
        keys: I32Slice(&data.mission_tasks[..]),
        table: &db.mission_tasks,
        id_col: db.mission_tasks.get_col(MissionTasksColumn::Uid).unwrap(),
        layout: AdapterLayout::Map,
    };
    Some(Api {
        data,
        embedded: SkillIDEmbedded { mission_tasks },
    })
}
