use std::{borrow::Cow, collections::BTreeSet, ops::Deref};

use assembly_fdb::{
    mem::MemContext,
    value::{Value, ValueType},
    FdbHash,
};
use latin1str::Latin1String;
use serde::Deserialize;

pub(super) enum ValueSet {
    /// The NULL value
    Nothing,
    /// A 32-bit signed integer
    Integer(BTreeSet<i32>),
    /// A 32-bit IEEE floating point number
    Float,
    /// A long string
    Text(BTreeSet<Latin1String>),
    /// A boolean
    Boolean { _true: bool, _false: bool },
    /// A 64 bit integer
    BigInt(BTreeSet<i64>),
    /// An (XML?) string
    VarChar,
}

impl ValueSet {
    pub(crate) fn contains(&self, pk: &Value<MemContext>) -> bool {
        match self {
            ValueSet::Nothing | ValueSet::Float | ValueSet::VarChar => false,
            ValueSet::Integer(s) => pk
                .into_opt_integer()
                .map(|i| s.contains(&i))
                .unwrap_or(false),
            ValueSet::Text(s) => pk.into_opt_text().map(|t| s.contains(t)).unwrap_or(false),
            ValueSet::Boolean { _true, _false } => match pk.into_opt_boolean() {
                Some(true) => *_true,
                Some(false) => *_false,
                None => false,
            },
            ValueSet::BigInt(s) => pk
                .into_opt_big_int()
                .map(|i| s.contains(&i))
                .unwrap_or(false),
        }
    }

    pub fn bucket_set(&self, bucket_count: usize) -> BTreeSet<usize> {
        match self {
            ValueSet::Integer(s) => s
                .iter()
                .map(FdbHash::hash)
                .map(|i: u32| i as usize % bucket_count)
                .collect(),
            ValueSet::Text(s) => s
                .iter()
                .map(|s| s.deref())
                .map(FdbHash::hash)
                .map(|i: u32| i as usize % bucket_count)
                .collect(),
            ValueSet::Boolean { _true, _false } => {
                let true_mod = 1 % bucket_count; // Might be reasonable that bool PKs are just one bucket
                match (_true, _false) {
                    (false, false) => BTreeSet::from([]),
                    (true, false) => BTreeSet::from([true_mod]),
                    (false, true) => BTreeSet::from([0]),
                    (true, true) => BTreeSet::from([0, true_mod]),
                }
            }
            ValueSet::BigInt(s) => s
                .iter()
                .map(FdbHash::hash)
                .map(|i: u32| i as usize % bucket_count)
                .collect(),
            ValueSet::Float | ValueSet::VarChar | ValueSet::Nothing => BTreeSet::new(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct TableQuery<'req_body, PKSet> {
    #[serde(default)]
    pub(super) pks: PKSet,
    #[serde(borrow, default)]
    pub(super) columns: BTreeSet<&'req_body str>,
}

impl<'req> TableQuery<'req, ValueSet> {
    fn de<T: Default + Deserialize<'req>>(
        body: &'req [u8],
        f: impl FnOnce(T) -> ValueSet,
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
                ValueSet::Text(
                    s.into_iter()
                        .map(Latin1String::encode)
                        .map(Cow::into_owned) // :(
                        .collect(),
                )
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
