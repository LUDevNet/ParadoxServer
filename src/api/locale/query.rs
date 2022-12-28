#![warn(missing_docs)]
use std::collections::BTreeSet;

use assembly_xml::localization::Key;
use serde::{
    de::{SeqAccess, Visitor},
    Deserialize, Serialize,
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
    Int(u32),
    Key(Key),
    String(String),
}

impl<'de> Deserialize<'de> for IntOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct __Visitor;

        use std::convert::TryFrom;

        impl<'de> Visitor<'de> for __Visitor {
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

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.contains('_') {
                    Ok(IntOrString::String(v.to_string()))
                } else {
                    match v.parse() {
                        Ok(int) => Ok(IntOrString::Int(int)),
                        Err(_) => Key::from_str(v)
                            .map(IntOrString::Key)
                            .map_err(|e| E::custom(e)),
                    }
                }
            }
        }

        deserializer.deserialize_any(__Visitor)
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

fn key_parts<E: serde::de::Error>(s: &str) -> Result<Vec<IntOrKey>, E> {
    let mut vec = Vec::new();
    for part in s.split('_') {
        vec.push(match part.parse() {
            Ok(i) => IntOrKey::Int(i),
            Err(_) => IntOrKey::Key(Key::from_str(part).map_err(E::custom)?),
        })
    }
    Ok(vec)
}

impl<'de> Deserialize<'de> for IntStringSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct __Visitor;

        impl<'de> Visitor<'de> for __Visitor {
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
                while let Some(elem) = seq.next_element::<IntOrString>()? {
                    match elem {
                        IntOrString::Int(i) => int_keys.insert(i),
                        IntOrString::Key(k) => str_keys.insert(k),
                        IntOrString::String(full) => full_keys.insert(full),
                    };
                }
                let mut vec_keys = Vec::with_capacity(full_keys.len());
                for full in full_keys {
                    vec_keys.push(CompositeKey {
                        parts: key_parts(&full)?,
                        full,
                    })
                }
                Ok(IntStringSet {
                    int_keys,
                    str_keys,
                    vec_keys,
                })
            }
        }

        deserializer.deserialize_seq(__Visitor)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use assembly_xml::localization::Key;

    use super::IntStringSet;

    #[test]
    fn test_deserialize() {
        assert_eq!(
            serde_json::from_str::<IntStringSet>(r#"[1,2,3,4,"a","b","c"]"#).unwrap(),
            IntStringSet {
                int_keys: BTreeSet::from([1, 2, 3, 4]),
                str_keys: ["a", "b", "c"]
                    .iter()
                    .copied()
                    .map(Key::from_str)
                    .map(Result::unwrap)
                    .collect::<BTreeSet<Key>>(),
                vec_keys: Vec::new()
            }
        );
    }
}
