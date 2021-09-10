use std::{convert::TryFrom, fmt};

use assembly_data::fdb::common::Latin1Str;
use serde::{Deserialize, Serialize};

#[repr(u8)]
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
#[serde(try_from = "u8", into = "u8")]
enum LocStatus {
    Missing = 0,
    InProgress = 1,
    Complete = 2,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct InvalidValueError;

impl std::error::Error for InvalidValueError {}

impl fmt::Display for InvalidValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid Value")
    }
}

impl TryFrom<u8> for LocStatus {
    type Error = InvalidValueError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Missing),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::Complete),
            _ => Err(InvalidValueError),
        }
    }
}

impl From<LocStatus> for u8 {
    fn from(l: LocStatus) -> Self {
        match l {
            LocStatus::Missing => 0,
            LocStatus::InProgress => 1,
            LocStatus::Complete => 2,
        }
    }
}

#[derive(Serialize)]
pub struct MissionTask<'a> {
    id: u32,
    #[serde(rename = "locStatus")]
    loc_status: u8,
    #[serde(rename = "taskType")]
    task_type: i32,
    target: i32,
    #[serde(rename = "targetGroup")]
    target_group: &'a Latin1Str,
    #[serde(rename = "targetValue")]
    target_value: i32,
    #[serde(rename = "taskParam1")]
    task_param1: &'a Latin1Str,
    #[serde(rename = "largeTaskIcon")]
    large_task_icon: &'a Latin1Str,
    #[serde(rename = "IconID")]
    icon_id: i32,
    uid: i32,
    #[serde(rename = "largeTaskIconID")]
    large_task_icon_id: i32,
    localize: bool,
    gate_version: &'a Latin1Str,
}
