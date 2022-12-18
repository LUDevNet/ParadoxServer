use std::{fmt, str};

use assembly_xml::localization::LocaleNode;
use serde::{
    ser::{SerializeMap, SerializeStruct},
    Serialize,
};

use super::adapter::Keys;

#[derive(Debug)]
pub(super) struct Pod<'a> {
    inner: &'a LocaleNode,
}

impl<'a> Pod<'a> {
    pub fn new(inner: &'a LocaleNode) -> Self {
        Self { inner }
    }
}

impl<'a> Serialize for Pod<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_struct("LocaleNode", 3)?;
        m.serialize_field("value", &self.inner.value)?;
        m.serialize_field("int_keys", &Keys::new(&self.inner.int_children))?;
        m.serialize_field("str_keys", &Keys::new(&self.inner.str_children))?;
        m.end()
    }
}

struct WithSuffix<'a, T> {
    suffix: &'a str,
    value: &'a T,
}

impl<'a, T> WithSuffix<'a, T> {
    pub fn new(value: &'a T, suffix: &'a str) -> Self {
        Self { suffix, value }
    }
}

impl<'a, T: fmt::Display + Serialize> Serialize for WithSuffix<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.suffix.is_empty() {
            self.value.serialize(serializer)
        } else {
            format!("{}{}", self.value, self.suffix).serialize(serializer)
        }
    }
}

pub(super) struct All<'a> {
    inner: &'a LocaleNode,
}

impl<'a> All<'a> {
    pub fn new(inner: &'a LocaleNode) -> Self {
        Self { inner }
    }

    pub fn new_inner(mut inner: &'a LocaleNode) -> (String, Self) {
        let mut suffix = String::new();
        loop {
            let v_count = usize::from(inner.value.is_some());
            let i_count = inner.int_children.len();
            let s_count = inner.str_children.len();

            let count = v_count + i_count + s_count;
            if count == 1 && s_count == 1 {
                // Flatten string keys into one string
                let (key, value) = inner.str_children.iter().next().unwrap();
                suffix.push('_');
                suffix.push_str(key);
                inner = value;
                continue;
            }
            break;
        }
        (suffix, Self { inner })
    }
}

impl<'a> Serialize for All<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let v_count = usize::from(self.inner.value.is_some());
        let i_count = self.inner.int_children.len();
        let s_count = self.inner.str_children.len();
        let count = v_count + i_count + s_count;

        let sub_count = i_count + s_count;
        if sub_count > 0 {
            let mut m = serializer.serialize_map(Some(count))?;
            if let Some(v) = &self.inner.value {
                m.serialize_entry(&"$value", v)?;
            }
            for (key, inner) in &self.inner.int_children {
                let value = All::new(inner);
                m.serialize_entry(&key, &value)?;
            }
            for (key, inner) in &self.inner.str_children {
                let (suffix, value) = All::new_inner(inner);
                m.serialize_entry(&WithSuffix::new(&key, &suffix), &value)?;
            }
            m.end()
        } else if let Some(v) = &self.inner.value {
            serializer.serialize_str(v)
        } else {
            serializer.serialize_none()
        }
    }
}

pub(super) enum Mode {
    /// Serialize the full subtree
    All,
    /// Serialize just this level
    Pod,
}

fn node_child<'a>(node: &'a LocaleNode, key: &str) -> Option<&'a LocaleNode> {
    if let Ok(key) = key.parse() {
        node.int_children.get(&key)
    } else {
        node.str_children.get(key)
    }
}

pub(super) fn select_node<'a>(
    mut node: &'a LocaleNode,
    mut rest: str::Split<'_, char>,
) -> Option<(&'a LocaleNode, Mode)> {
    for seg in &mut rest {
        if let Some(new) = node_child(node, seg) {
            node = new;
            continue;
        }
        return match (seg, rest.next()) {
            ("$all", None) => Some((node, Mode::All)),
            ("", None) => Some((node, Mode::Pod)),
            _ => None,
        };
    }
    Some((node, Mode::Pod))
}
