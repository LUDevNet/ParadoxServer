use crate::api::PercentDecoded;
use std::str;

pub(super) static REV_APIS: &[&str; 11] = &[
    "activity",
    "behaviors",
    "component_types",
    "faction",
    "gate_version",
    "loot_table_index",
    "mission_types",
    "missions",
    "objects",
    "object_types",
    "skill_ids",
];

#[derive(Debug)]
pub(crate) enum Route {
    Base,
    Activities,
    ActivityById(i32),
    BehaviorById(i32),
    ComponentTypes,
    ComponentTypeById(i32),
    ComponentTypeByIdAndCid(i32, i32),
    Factions,
    FactionById(i32),
    LootTableIndexById(i32),
    MissionTypes,
    MissionTypesFull,
    MissionTypeByTy(PercentDecoded),
    MissionTypeBySubTy(PercentDecoded, PercentDecoded),
    Objects,
    ObjectById(i32),
    ObjectsSearchIndex,
    ObjectTypes,
    ObjectTypeByName(PercentDecoded),
    SkillById(i32),
    GateVersions,
    GateVersionByName(PercentDecoded),
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

    pub(crate) fn from_parts(mut parts: str::Split<'_, char>) -> Result<Self, ()> {
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
            Some("faction" | "factions") => match parts.next() {
                None => Ok(Self::Factions),
                Some("") => match parts.next() {
                    None => Ok(Self::Factions),
                    Some(_) => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(id) => match parts.next() {
                        None => Ok(Self::FactionById(id)),
                        Some("") => match parts.next() {
                            Some(_) => Err(()),
                            None => Ok(Self::FactionById(id)),
                        },
                        Some(_) => Err(()),
                    },
                    Err(_) => Err(()),
                },
            },
            Some("gate_version" | "gate-versions") => match parts.next() {
                None => Ok(Self::GateVersions),
                Some("") => match parts.next() {
                    None => Ok(Self::GateVersions),
                    Some(_) => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(name) => match parts.next() {
                        None => Ok(Self::GateVersionByName(name)),
                        Some("") => match parts.next() {
                            None => Ok(Self::GateVersionByName(name)),
                            Some(_) => Err(()),
                        },
                        Some(_) => Err(()),
                    },
                    Err(_) => Err(()),
                },
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
                None => Ok(Self::Objects),
                Some("") => match parts.next() {
                    None => Ok(Self::Objects),
                    Some(_) => Err(()),
                },
                Some("search_index" | "search-index") => match parts.next() {
                    None => Ok(Self::ObjectsSearchIndex),
                    Some("") => match parts.next() {
                        None => Ok(Self::ObjectsSearchIndex),
                        _ => Err(()),
                    },
                    Some(_) => Err(()),
                },
                Some(key) => match key.parse() {
                    Ok(lot) => match parts.next() {
                        None => Ok(Self::ObjectById(lot)),
                        Some("") => match parts.next() {
                            None => Ok(Self::ObjectById(lot)),
                            Some(_) => Err(()),
                        },
                        Some(_) => Err(()),
                    },
                    Err(_) => Err(()),
                },
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
