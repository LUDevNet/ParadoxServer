use std::{fmt, str};

use assembly_xml::localization::{Key, LocaleNode};
use http::StatusCode;
use hyper::body::{Buf, Bytes};
use serde::{
    ser::{SerializeMap, SerializeStruct},
    Serialize,
};

use crate::data::locale::LocaleRoot;

use self::query::{CompositeKey, IntOrKey, IntOrStr, IntStringSet};

use super::{adapter::Keys, Accept, ApiFuture, RestPath};

mod query;

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

fn node_child(node: &LocaleNode, key: Key) -> Option<&LocaleNode> {
    if let Ok(key) = key.parse() {
        node.int_children.get(&key)
    } else {
        node.str_children.get(&key)
    }
}

pub(super) fn select_node<'a>(
    mut node: &'a LocaleNode,
    mut rest: RestPath,
) -> Option<(&'a LocaleNode, Mode)> {
    for seg in &mut rest.0 {
        if let Some(new) = Key::from_str(seg)
            .ok()
            .and_then(|key| node_child(node, key))
        {
            node = new;
            continue;
        }
        return match (seg, rest.0.next()) {
            ("$all", None) => Some((node, Mode::All)),
            ("", None) => Some((node, Mode::Pod)),
            _ => None,
        };
    }
    Some((node, Mode::Pod))
}

struct Query<'l, 'q> {
    layers: &'q [IntStringSet],
    node: &'l LocaleNode,
}

fn node_get_vec<'l>(mut node: &'l LocaleNode, s: &CompositeKey) -> Option<&'l LocaleNode> {
    for part in &s.parts {
        let child = match part {
            IntOrKey::Int(i) => node.int_children.get(i),
            IntOrKey::Key(s) => node.str_children.get(s),
        };
        match child {
            Some(child) => node = child,
            None => return None,
        }
    }
    Some(node)
}

impl<'l, 'q> Serialize for Query<'l, 'q> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some((first, rest)) = self.layers.split_first() {
            let int_nodes = first.int_keys.iter().filter_map(|int| {
                self.node
                    .int_children
                    .get(int)
                    .map(|n| (IntOrStr::Int(*int), n))
            });
            let str_nodes = first
                .str_keys
                .iter()
                .filter_map(|s| self.node.str_children.get(s).map(|n| (IntOrStr::Str(s), n)));
            let vec_nodes = first
                .vec_keys
                .iter()
                .filter_map(|s| node_get_vec(self.node, s).map(|n| (IntOrStr::Str(&s.full), n)));
            let nodes = int_nodes.chain(str_nodes).chain(vec_nodes);
            serializer.collect_map(nodes.map(|(k, node)| (k, Query { layers: rest, node })))
        } else if let Some(v) = &self.node.value {
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
        let query_layers: Vec<query::IntStringSet> = match serde_json::from_reader(rdr) {
            Ok(q) => q,
            Err(e) => {
                return super::reply_400(accept, "Failed to parse body as JSON", e);
            }
        };
        let rest = key.split('_');
        let query = match select_node(root.as_ref(), RestPath(rest)) {
            Some((node, _)) => Query {
                layers: &query_layers[..],
                node,
            },
            None => return Ok(super::reply_404()),
        };
        super::reply(accept, &query, StatusCode::OK)
    })
}
