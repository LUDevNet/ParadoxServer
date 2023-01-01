use std::{collections::btree_map, fmt, ops::Deref, str};

use assembly_xml::localization::{Interner, Key, LocaleNode, LocaleNodeRef};
use http::StatusCode;
use hyper::body::{Buf, Bytes};
use serde::{
    de::DeserializeSeed,
    ser::{SerializeMap, SerializeStruct},
    Serialize,
};

use crate::data::locale::LocaleRoot;

use self::query::{CompositeKey, IntOrKey, IntOrStr, IntStringSet, VecIntStringSetSeed};

use super::{adapter::Keys, Accept, ApiFuture, RestPath};

mod query;

#[derive(Debug, Clone)]
pub(super) struct PodStrKeys<'a, 's> {
    map: btree_map::Keys<'a, Key, LocaleNode>,
    strs: &'s Interner,
}

impl<'a, 's> Serialize for PodStrKeys<'a, 's> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let map = &self.map;
        let strs = self.strs;
        let mapper = |id: &Key| strs.lookup(*id);
        serializer.collect_seq(map.clone().map(mapper))
    }
}

pub(super) struct Pod<'a, 's> {
    inner: LocaleNodeRef<'a, 's>,
}

impl<'a, 's> Pod<'a, 's> {
    pub fn new(inner: LocaleNodeRef<'a, 's>) -> Self {
        Self { inner }
    }

    pub fn str_keys(&self) -> PodStrKeys<'a, 's> {
        PodStrKeys {
            map: self.inner.node().str_children.keys(),
            strs: self.inner.strs(),
        }
    }
}

impl<'a, 's> Serialize for Pod<'a, 's> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_struct("LocaleNode", 3)?;
        m.serialize_field("value", &self.inner.value())?;
        m.serialize_field("int_keys", &Keys::new(&self.inner.node().int_children))?;
        m.serialize_field("str_keys", &self.str_keys())?;
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

pub(super) struct All<'a, 's> {
    inner: LocaleNodeRef<'a, 's>,
}

impl<'a, 's: 'a> All<'a, 's> {
    pub fn new(inner: LocaleNodeRef<'a, 's>) -> Self {
        Self { inner }
    }

    pub fn new_inner(mut inner: LocaleNodeRef<'a, 's>) -> (String, Self) {
        let mut suffix = String::new();
        let node = inner.node();
        loop {
            let v_count = usize::from(inner.value().is_some());
            let i_count = node.int_children.len();
            let s_count = node.str_children.len();

            let count = v_count + i_count + s_count;
            if count == 1 && s_count == 1 {
                // Flatten string keys into one string
                let mut iter = inner.str_child_iter();
                let (key, value) = iter.next().unwrap();
                suffix.push('_');
                suffix.push_str(&key);
                inner = value;
                continue;
            }
            break;
        }
        (suffix, Self { inner })
    }
}

impl<'a, 's> Serialize for All<'a, 's> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let v_count = usize::from(self.inner.value().is_some());
        let i_count = self.inner.node().int_children.len();
        let s_count = self.inner.node().str_children.len();
        let count = v_count + i_count + s_count;

        let sub_count = i_count + s_count;
        if sub_count > 0 {
            let mut m = serializer.serialize_map(Some(count))?;
            if let Some(v) = self.inner.value() {
                m.serialize_entry(&"$value", v)?;
            }
            for (key, inner) in self.inner.int_child_iter() {
                let value = All::new(inner);
                m.serialize_entry(&key, &value)?;
            }
            for (key, inner) in self.inner.str_child_iter() {
                let (suffix, value) = All::new_inner(inner);
                m.serialize_entry(&WithSuffix::new(&key.deref(), &suffix), &value)?;
            }
            m.end()
        } else if let Some(v) = &self.inner.value() {
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

/*
fn node_child(node: &LocaleNode, key: Key) -> Option<&LocaleNode> {
    if let Ok(key) = key.parse() {
        node.int_children.get(&key)
    } else {
        node.str_children.get(&key)
    }
}
*/

pub(super) fn select_node<'a, 's>(
    mut node: LocaleNodeRef<'a, 's>,
    mut rest: RestPath,
) -> Option<(LocaleNodeRef<'a, 's>, Mode)> {
    for seg in &mut rest.0 {
        if let Ok(id) = seg.parse() {
            if let Some(next) = node.get_int(id) {
                node = next;
                continue;
            }
        }
        if let Some(key) = node.strs().get(seg) {
            if let Some(next) = node.get_str(key) {
                node = next;
                continue;
            }
        }
        return match (seg, rest.0.next()) {
            ("$all", None) => Some((node, Mode::All)),
            ("", None) => Some((node, Mode::Pod)),
            _ => None,
        };
    }
    Some((node, Mode::Pod))
}

struct Query<'l, 's, 'q> {
    layers: &'q [IntStringSet],
    node: LocaleNodeRef<'l, 's>,
}

fn node_get_vec<'l, 's>(
    mut node: LocaleNodeRef<'l, 's>,
    s: &CompositeKey,
) -> Option<LocaleNodeRef<'l, 's>> {
    for &part in &s.parts {
        let child = match part {
            IntOrKey::Int(i) => node.get_int(i),
            IntOrKey::Key(s) => node.get_str(s),
        };
        match child {
            Some(child) => node = child,
            None => return None,
        }
    }
    Some(node)
}

impl<'l, 's, 'q> Serialize for Query<'l, 's, 'q> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some((first, rest)) = self.layers.split_first() {
            let int_nodes = first
                .int_keys
                .iter()
                .filter_map(|&int| self.node.get_int(int).map(|n| (IntOrStr::Int(int), n)));
            let str_nodes = first.str_keys.iter().filter_map(|&s| {
                let string = self.node.strs().lookup(s);
                self.node.get_str(s).map(|n| (IntOrStr::Str(string), n))
            });
            let node = &self.node;
            let vec_nodes = first
                .vec_keys
                .iter()
                .filter_map(|s| node_get_vec(node.clone(), s).map(|n| (IntOrStr::Str(&s.full), n)));
            let nodes = int_nodes.chain(str_nodes).chain(vec_nodes);
            serializer.collect_map(nodes.map(|(k, node)| (k, Query { layers: rest, node })))
        } else if let Some(v) = self.node.value() {
            serializer.serialize_str(v)
        } else {
            serializer.serialize_none()
        }
    }
}

/// Get data from `locale.xml`
pub(super) fn locale_query<ReqBody>(
    root: &LocaleRoot,
    accept: Accept,
    rest: RestPath,
    body: ReqBody,
) -> ApiFuture
where
    ReqBody: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
    ReqBody::Error: fmt::Display,
{
    let key = rest.join('_');
    let root = root.root.clone();
    ApiFuture::boxed(async move {
        let rdr = match hyper::body::aggregate(body).await {
            Ok(buf) => buf.reader(),
            Err(e) => {
                return super::reply_400(accept, "Failed to decode body", e);
            }
        };
        let node = root.node();
        let strs = node.strs();
        let mut de = serde_json::Deserializer::from_reader(rdr);
        let query_layers: Vec<query::IntStringSet> =
            match VecIntStringSetSeed(strs).deserialize(&mut de) {
                Ok(q) => q,
                Err(e) => {
                    return super::reply_400(accept, "Failed to parse body as JSON", e);
                }
            };
        let rest = key.split('_');
        let query = match select_node(node, RestPath(rest)) {
            Some((node, _)) => Query {
                layers: &query_layers[..],
                node,
            },
            None => return Ok(super::reply_404()),
        };
        super::reply(accept, &query, StatusCode::OK)
    })
}
