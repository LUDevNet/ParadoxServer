use assembly_data::xml::localization::LocaleNode;
use paradox_typed_db::typed_rows::TypedRow;
use serde::{ser::SerializeMap, Serialize};

pub(crate) trait FindHash {
    fn find_hash(&self, v: i32) -> Option<i32>;
}

impl<T: FindHash> FindHash for &T {
    fn find_hash(&self, v: i32) -> Option<i32> {
        (*self).find_hash(v)
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct IdentityHash;

impl FindHash for IdentityHash {
    fn find_hash(&self, v: i32) -> Option<i32> {
        Some(v)
    }
}

#[derive(Clone)]
pub(crate) struct TypedTableIterAdapter<'a, 'b, R: TypedRow<'a, 'b>, F, K> {
    pub index: F,
    pub keys: K,
    pub table: &'b R::Table,
    pub id_col: usize,
}

impl<'a, 'b, R: TypedRow<'a, 'b>> TypedTableIterAdapter<'a, 'b, R, IdentityHash, &'b [i32]> {
    pub fn new(table: &'b R::Table, keys: &'b [i32]) -> Self {
        Self {
            index: IdentityHash,
            keys,
            table,
            id_col: 0,
        }
    }
}

impl<'b, 'a: 'b, R: TypedRow<'a, 'b> + 'b, F, K> Serialize
    for TypedTableIterAdapter<'a, 'b, R, F, K>
where
    R: Serialize,
    F: FindHash + Copy + 'b,
    K: IntoIterator<Item = &'b i32> + Copy + 'b,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.to_iter(self.id_col))
    }
}

impl<'b, 'a: 'b, R, F, K> TypedTableIterAdapter<'a, 'b, R, F, K>
where
    R: TypedRow<'a, 'b> + 'b,
{
    pub(crate) fn to_iter(&self, id_col: usize) -> impl Iterator<Item = (i32, R)> + 'b
    where
        F: FindHash + Copy + 'b,
        K: IntoIterator<Item = &'b i32> + Copy + 'b,
    {
        let t: &'b R::Table = self.table;
        let i = self.index;
        let iter = self.keys.into_iter().copied();
        let mapper = move |key| {
            let index = i.find_hash(key)?;
            let r = R::get(t, index, key, id_col)?;
            Some((key, r))
        };
        iter.filter_map(mapper)
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

#[derive(Debug, Serialize)]
pub(super) struct LocalePod<'a> {
    pub value: Option<&'a str>,
    pub int_keys: Vec<u32>,
    pub str_keys: Vec<&'a str>,
}

pub(super) struct LocaleAll<'a> {
    inner: &'a LocaleNode,
}

impl<'a> LocaleAll<'a> {
    pub fn new(inner: &'a LocaleNode) -> Self {
        Self { inner }
    }
}

impl<'a> Serialize for LocaleAll<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let v_count = if self.inner.value.is_some() { 1 } else { 0 };
        let i_count = self.inner.int_children.len();
        let s_count = self.inner.str_children.len();
        let count = v_count + i_count + s_count;

        if i_count + s_count > 0 {
            let mut m = serializer.serialize_map(Some(count))?;
            if let Some(v) = &self.inner.value {
                m.serialize_entry(&"$value", v)?;
            }
            for (key, inner) in &self.inner.int_children {
                m.serialize_entry(key, &Self { inner })?;
            }
            for (key, inner) in &self.inner.str_children {
                m.serialize_entry(key, &Self { inner })?;
            }
            m.end()
        } else if let Some(v) = &self.inner.value {
            serializer.serialize_str(v)
        } else {
            serializer.serialize_none()
        }
    }
}
