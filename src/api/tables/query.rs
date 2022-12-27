use std::{borrow::Cow, collections::BTreeSet};

use assembly_fdb::value::ValueType;
use latin1str::{Latin1Str, Latin1String};
use serde::Deserialize;

pub(super) enum ValueSet<'req> {
    /// The NULL value
    Nothing,
    /// A 32-bit signed integer
    Integer(BTreeSet<i32>),
    /// A 32-bit IEEE floating point number
    Float,
    /// A long string
    Text(BTreeSet<Cow<'req, Latin1Str>>),
    /// A boolean
    Boolean { _true: bool, _false: bool },
    /// A 64 bit integer
    BigInt(BTreeSet<i64>),
    /// An (XML?) string
    VarChar,
}

#[derive(Deserialize)]
pub(super) struct TableQuery<'req_body, PKSet> {
    #[serde(default)]
    pub(super) pks: PKSet,
    #[serde(borrow, default)]
    pub(super) columns: BTreeSet<&'req_body str>,
}

impl<'req> TableQuery<'req, ValueSet<'req>> {
    fn de<T: Default + Deserialize<'req>>(
        body: &'req [u8],
        f: impl FnOnce(T) -> ValueSet<'req>,
    ) -> Result<Self, serde_json::Error> {
        serde_json::from_slice::<TableQuery<'req, T>>(body).map(|tq| TableQuery {
            pks: f(tq.pks),
            columns: tq.columns,
        })
    }

    pub fn new(ty: ValueType, body: &'req [u8]) -> Result<Self, serde_json::Error> {
        match ty {
            ValueType::Nothing => Self::de::<()>(body, |()| ValueSet::Nothing),
            ValueType::Integer => Self::de::<BTreeSet<i32>>(body, ValueSet::Integer),
            ValueType::Float => Self::de::<()>(body, |()| ValueSet::Float),
            ValueType::Text => Self::de::<BTreeSet<&'req str>>(body, |s| {
                ValueSet::Text(s.into_iter().map(Latin1String::encode).collect())
            }),
            ValueType::Boolean => Self::de::<BTreeSet<bool>>(body, |s| ValueSet::Boolean {
                _true: s.contains(&true),
                _false: s.contains(&false),
            }),
            ValueType::BigInt => Self::de::<BTreeSet<i64>>(body, ValueSet::BigInt),
            ValueType::VarChar => Self::de::<()>(body, |()| ValueSet::VarChar),
        }
    }
}
