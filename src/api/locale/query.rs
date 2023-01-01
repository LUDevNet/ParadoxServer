#![warn(missing_docs)]
use std::collections::BTreeSet;

use assembly_xml::localization::{Interner, Key};
use serde::{
    de::{DeserializeSeed, SeqAccess, Visitor},
    Serialize,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum IntOrStr<'a> {
    Int(u32),
    Str(&'a str),
}

impl Serialize for IntOrStr<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            Self::Int(i) => i.serialize(serializer),
            Self::Str(s) => s.serialize(serializer),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IntOrKey {
    Int(u32),
    Key(Key),
}

enum IntOrString {
    Ignore,
    Int(u32),
    Key(Key),
    String(String),
}

struct IntOrStringSeed<'s> {
    strs: &'s Interner,
}

impl<'s, 'de> DeserializeSeed<'de> for IntOrStringSeed<'s> {
    type Value = IntOrString;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct __Visitor<'s>(&'s Interner);

        use std::convert::TryFrom;

        impl<'s, 'de> Visitor<'de> for __Visitor<'s> {
            type Value = IntOrString;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or an int")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                u32::try_from(v)
                    .map(IntOrString::Int)
                    .map_err(|_| E::custom("Integer out of range"))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                u32::try_from(v)
                    .map(IntOrString::Int)
                    .map_err(|_| E::custom("Integer out of range"))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(IntOrString::Ignore)
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.contains('_') {
                    Ok(IntOrString::String(v.to_string()))
                } else {
                    match v.parse() {
                        Ok(int) => Ok(IntOrString::Int(int)),
                        Err(_) => Ok(self
                            .0
                            .get(v)
                            .map(IntOrString::Key)
                            .unwrap_or(IntOrString::Ignore)),
                    }
                }
            }
        }

        deserializer.deserialize_any(__Visitor(self.strs))
    }
}

/// A Set that can hold [i32] and [String] values at the same time.
///
/// Note that elements are never parsed, so `"1"` and `1` can both
/// be in the set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct IntStringSet {
    pub int_keys: BTreeSet<u32>,
    pub str_keys: BTreeSet<Key>,
    pub vec_keys: Vec<CompositeKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct CompositeKey {
    pub(super) parts: Vec<IntOrKey>,
    pub(super) full: String,
}

fn key_parts(s: &str, strs: &Interner) -> Option<Vec<IntOrKey>> {
    let mut vec = Vec::new();
    for part in s.split('_') {
        vec.push(match part.parse() {
            Ok(i) => IntOrKey::Int(i),
            Err(_) => strs.get(part).map(IntOrKey::Key)?,
        });
    }
    Some(vec)
}

struct IntStringSetSeed<'s>(&'s Interner);

impl<'s, 'de> DeserializeSeed<'de> for IntStringSetSeed<'s> {
    type Value = IntStringSet;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct __Visitor<'s>(&'s Interner);

        impl<'s, 'de> Visitor<'de> for __Visitor<'s> {
            type Value = IntStringSet;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a set of strings or numbers")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut int_keys = BTreeSet::<u32>::new();
                let mut str_keys = BTreeSet::<Key>::new();
                let mut full_keys = BTreeSet::<String>::new();
                while let Some(elem) = seq.next_element_seed(IntOrStringSeed { strs: self.0 })? {
                    match elem {
                        IntOrString::Int(i) => int_keys.insert(i),
                        IntOrString::Key(k) => str_keys.insert(k),
                        IntOrString::String(full) => full_keys.insert(full),
                        IntOrString::Ignore => false,
                    };
                }
                let mut vec_keys = Vec::with_capacity(full_keys.len());
                for full in full_keys {
                    if let Some(parts) = key_parts(&full, self.0) {
                        vec_keys.push(CompositeKey { parts, full })
                    }
                }
                Ok(IntStringSet {
                    int_keys,
                    str_keys,
                    vec_keys,
                })
            }
        }

        deserializer.deserialize_seq(__Visitor(self.0))
    }
}

pub(super) struct VecIntStringSetSeed<'s>(pub &'s Interner);

impl<'s, 'de> DeserializeSeed<'de> for VecIntStringSetSeed<'s> {
    type Value = Vec<IntStringSet>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct __Visitor<'s>(&'s Interner);

        impl<'s, 'de> Visitor<'de> for __Visitor<'s> {
            type Value = Vec<IntStringSet>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a sequence of sequences")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                loop {
                    let next = seq.next_element_seed(IntStringSetSeed(self.0))?;
                    let Some(next) = next else { break };
                    vec.push(next);
                }
                Ok(vec)
            }
        }

        deserializer.deserialize_seq(__Visitor(self.0))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use assembly_xml::localization::Interner;
    use serde::de::DeserializeSeed;

    use crate::api::locale::query::IntStringSetSeed;

    use super::IntStringSet;

    #[test]
    fn test_deserialize() {
        let mut interner = Interner::with_capacity(100);
        let key_a = interner.intern("a");
        let key_b = interner.intern("b");
        let key_c = interner.intern("c");
        let mut de = serde_json::Deserializer::from_str(r#"[1,2,3,4,"a","b","c"]"#);
        assert_eq!(
            IntStringSetSeed(&interner).deserialize(&mut de).unwrap(),
            IntStringSet {
                int_keys: BTreeSet::from([1, 2, 3, 4]),
                str_keys: [key_a, key_b, key_c].iter().copied().collect(),
                vec_keys: Vec::new()
            }
        );
    }
}
