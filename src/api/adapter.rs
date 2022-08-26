use std::collections::{BTreeMap, HashMap};
use std::iter::Copied;
use std::slice::Iter;

use assembly_xml::localization::LocaleNode;
use paradox_typed_db::TypedRow;
use serde::{ser::SerializeMap, Serialize};

pub(crate) trait FindHash {
    fn find_hash(&self, v: i32) -> Option<i32>;
}

impl<T: FindHash> FindHash for &T {
    fn find_hash(&self, v: i32) -> Option<i32> {
        (*self).find_hash(v)
    }
}

impl FindHash for BTreeMap<i32, i32> {
    fn find_hash(&self, v: i32) -> Option<i32> {
        self.get(&v).copied()
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct IdentityHash;

impl FindHash for IdentityHash {
    fn find_hash(&self, v: i32) -> Option<i32> {
        Some(v)
    }
}

#[derive(Debug, Copy, Clone)]
pub(super) struct I32Slice<'b>(pub(crate) &'b [i32]);

impl<'b> IntoIterator for I32Slice<'b> {
    type IntoIter = Copied<Iter<'b, i32>>;
    type Item = i32;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AdapterLayout {
    Seq,
    Map,
}

#[derive(Clone)]
pub(crate) struct TypedTableIterAdapter<'a, 'b, R: TypedRow<'a, 'b>, F, K> {
    /// A structure mapping IDs to primary keys
    pub index: F,
    pub keys: K,
    pub table: &'b R::Table,
    /// This needs to be the column that is the input to `F`
    pub id_col: usize,
    pub layout: AdapterLayout,
}

impl<'a, 'b, R: TypedRow<'a, 'b>> TypedTableIterAdapter<'a, 'b, R, IdentityHash, I32Slice<'b>> {
    pub fn new(table: &'b R::Table, keys: &'b [i32]) -> Self {
        Self {
            index: IdentityHash,
            keys: I32Slice(keys),
            table,
            id_col: 0,
            layout: AdapterLayout::Map,
        }
    }
}

impl<'b, 'a: 'b, R: TypedRow<'a, 'b> + 'b, F, K> Serialize
    for TypedTableIterAdapter<'a, 'b, R, F, K>
where
    R: Serialize,
    F: FindHash + Copy + 'b,
    K: IntoIterator<Item = i32> + Clone + 'b,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.layout {
            AdapterLayout::Seq => serializer.collect_seq(self.to_iter(self.id_col).map(|(_, r)| r)),
            AdapterLayout::Map => serializer.collect_map(self.to_iter(self.id_col)),
        }
    }
}

pub(crate) struct TableMultiIter<'a, 'b, R, K, F>
where
    K: Iterator<Item = i32>,
    F: FindHash,
    R: TypedRow<'a, 'b>,
{
    pub(crate) index: F,
    pub(crate) key_iter: K,
    pub(crate) table: &'b R::Table,
    pub(crate) id_col: usize,
}

impl<'a, 'b, R, K, F> Iterator for TableMultiIter<'a, 'b, R, K, F>
where
    K: Iterator<Item = i32>,
    F: FindHash,
    R: TypedRow<'a, 'b>,
{
    type Item = (i32, R);

    fn next(&mut self) -> Option<Self::Item> {
        for key in &mut self.key_iter {
            if let Some(hash) = self.index.find_hash(key) {
                if let Some(r) = R::get(self.table, hash, key, self.id_col) {
                    return Some((key, r));
                }
            }
        }
        None
    }
}

impl<'b, 'a: 'b, R, F, K> TypedTableIterAdapter<'a, 'b, R, F, K>
where
    R: TypedRow<'a, 'b> + 'b,
{
    pub(crate) fn to_iter(&self, id_col: usize) -> TableMultiIter<'a, 'b, R, K::IntoIter, F>
    where
        F: FindHash + Copy + 'b,
        K: IntoIterator<Item = i32> + Clone + 'b,
    {
        TableMultiIter {
            index: self.index,
            key_iter: self.keys.clone().into_iter(),
            table: self.table,
            id_col,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct LocaleTableAdapter<'a> {
    node: &'a LocaleNode,
    keys: &'a [i32],
}

impl<'a> LocaleTableAdapter<'a> {
    pub fn new(node: &'a LocaleNode, keys: &'a [i32]) -> Self {
        Self { node, keys }
    }
}

impl<'a> Serialize for LocaleTableAdapter<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut m = serializer.serialize_map(None)?;
        for &key in self.keys {
            if key >= 0 {
                if let Some(node) = self.node.int_children.get(&(key as u32)) {
                    m.serialize_entry(&key, &node.get_keys())?;
                }
            }
        }
        m.end()
    }
}

/// [Serialize] adapter that serializes a map as the sequence of its keys
pub(crate) struct Keys<M> {
    inner: M,
}

impl<M> Keys<M> {
    pub fn new(inner: M) -> Self {
        Self { inner }
    }
}

impl<'a, K: Serialize, V> Serialize for Keys<&'a BTreeMap<K, V>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.inner.keys())
    }
}

impl<'a, K: Serialize, V> Serialize for Keys<&'a HashMap<K, V>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.inner.keys())
    }
}
