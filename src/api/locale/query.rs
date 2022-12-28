#![warn(missing_docs)]
use std::collections::BTreeSet;

use serde::{
    de::{SeqAccess, Visitor},
    Deserialize,
};

/// A Set that can hold [i32] and [String] values at the same time.
///
/// Note that elements are never parsed, so `"1"` and `1` can both
/// be in the set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct IntStringSet {
    pub int_keys: BTreeSet<i32>,
    pub str_keys: BTreeSet<String>, // owned for now
}

impl<'de> Deserialize<'de> for IntStringSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum IntOrString {
            Int(i32),
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
                        i32::try_from(v)
                            .map(IntOrString::Int)
                            .map_err(|_| E::custom("Integer out of range"))
                    }

                    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        i32::try_from(v)
                            .map(IntOrString::Int)
                            .map_err(|_| E::custom("Integer out of range"))
                    }

                    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        Ok(IntOrString::String(v.to_string()))
                    }

                    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        Ok(IntOrString::String(v))
                    }
                }

                deserializer.deserialize_any(__Visitor)
            }
        }

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
                let mut set = IntStringSet::default();
                while let Some(elem) = seq.next_element::<IntOrString>()? {
                    match elem {
                        IntOrString::Int(i) => set.int_keys.insert(i),
                        IntOrString::String(s) => set.str_keys.insert(s),
                    };
                }
                Ok(set)
            }
        }

        deserializer.deserialize_seq(__Visitor)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::IntStringSet;

    #[test]
    fn test_deserialize() {
        assert_eq!(
            serde_json::from_str::<IntStringSet>(r#"[1,2,3,4,"a","b","c"]"#).unwrap(),
            IntStringSet {
                int_keys: BTreeSet::from([1, 2, 3, 4]),
                str_keys: ["a", "b", "c"]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<BTreeSet<String>>(),
            }
        );
    }
}
