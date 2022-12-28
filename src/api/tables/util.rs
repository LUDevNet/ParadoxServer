use std::{
    borrow::Cow,
    collections::{btree_set, BTreeSet},
    iter,
    marker::PhantomData,
    slice,
};

use assembly_fdb::{
    mem::{iter::TableRowIter, Bucket, FieldIter, MemContext, Row, RowHeaderIter, Table},
    value::Value,
};
use serde::Serialize;

use super::{query::ValueSet, FastContext};

pub(super) trait AsRowIter<'a> {
    type AsIter<'b>: Iterator<Item = Row<'a>> + 'b
    where
        Self: 'b;
    fn as_row_iter(&self) -> Self::AsIter<'_>;
}

impl<'a> AsRowIter<'a> for Table<'a> {
    type AsIter<'b> = TableRowIter<'a>
    where
        Self: 'b;

    fn as_row_iter(&self) -> Self::AsIter<'_> {
        self.row_iter()
    }
}

pub(super) trait AsColValIter<'a> {
    type AsIter<'b>: Iterator<Item = ColValPair<'a>> + 'b
    where
        Self: 'b;
    fn as_cv_iter<'b>(&'b self, row: Row<'a>) -> Self::AsIter<'b>;
}

impl<'a> AsColValIter<'a> for Vec<Cow<'a, str>> {
    type AsIter<'b> = ColValIter<'a, 'b> where Self: 'b;

    fn as_cv_iter<'b>(&'b self, row: Row<'a>) -> Self::AsIter<'b> {
        self.iter().cloned().zip(row.field_iter())
    }
}

type ColValIter<'a, 'b> = iter::Zip<iter::Cloned<slice::Iter<'b, Cow<'a, str>>>, FieldIter<'a>>;
#[allow(dead_code)] // false positive
type ColValPair<'a> = (Cow<'a, str>, Value<MemContext<'a>>);

pub(super) struct PartialColValIterSpec<'a> {
    indices: Vec<usize>,
    names: Vec<Cow<'a, str>>,
}

impl<'a> PartialColValIterSpec<'a> {
    pub(crate) fn new(names: Vec<Cow<'a, str>>, columns: &BTreeSet<&str>) -> Self {
        Self {
            indices: names
                .iter()
                .map(Cow::as_ref)
                .enumerate()
                .filter_map(|(i, name)| match columns.contains(name) {
                    true => Some(i),
                    false => None,
                })
                .collect::<Vec<_>>(),
            names,
        }
    }
}

impl<'a> AsColValIter<'a> for PartialColValIterSpec<'a> {
    type AsIter<'b> = PartialColValIter<'a, 'b> where Self: 'b;

    fn as_cv_iter<'b>(&'b self, row: Row<'a>) -> Self::AsIter<'b> {
        PartialColValIter {
            index: self.indices.iter().copied(),
            names: &self.names,
            row,
        }
    }
}

///
///
/// Lifetimes:
/// - `'a`: The [Database]
/// - `'b`: The [PartialColValIterSpec]
pub(super) struct PartialColValIter<'a, 'b> {
    index: iter::Copied<slice::Iter<'b, usize>>,
    row: Row<'a>,
    names: &'b [Cow<'a, str>],
}

impl<'a, 'b> Iterator for PartialColValIter<'a, 'b> {
    type Item = ColValPair<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.index.next();
        n.and_then(|index| self.names.get(index).cloned().zip(self.row.field_at(index)))
    }
}

struct OutRow<'a, 'b, AsIter: AsColValIter<'a>> {
    inner: &'b AsIter,
    row: Row<'a>,
}

impl<'a, 'b, AsIter: AsColValIter<'a> + 'b> Serialize for OutRow<'a, 'b, AsIter> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.inner.as_cv_iter(self.row))
    }
}

pub(super) struct RowIter<'a, FR, FC>
where
    FR: AsRowIter<'a>,
    FC: AsColValIter<'a>,
{
    to_rows: FR,
    to_cols: FC,
    _p: PhantomData<&'a ()>,
}

impl<'a, FR, FC> RowIter<'a, FR, FC>
where
    FR: AsRowIter<'a>,
    FC: AsColValIter<'a>,
{
    pub(crate) fn new(to_rows: FR, to_cols: FC) -> Self {
        Self {
            to_rows,
            to_cols,
            _p: PhantomData,
        }
    }
}

impl<'a, FR, FC> Serialize for RowIter<'a, FR, FC>
where
    FR: AsRowIter<'a>,
    FC: AsColValIter<'a>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.to_rows.as_row_iter().map(|row| OutRow {
            inner: &self.to_cols,
            row,
        }))
    }
}

pub(super) struct MultiPKFilterSpec<'a> {
    table: Table<'a>,
    buckets: BTreeSet<usize>,
    gate: ValueSet,
}

impl<'a> MultiPKFilterSpec<'a> {
    pub(crate) fn new(table: Table<'a>, pks: ValueSet) -> Self {
        let buckets = pks.bucket_set(table.bucket_count());
        Self {
            table,
            buckets,
            gate: pks,
        }
    }
}

struct PartialBucketIter<'a, 'b> {
    table: Table<'a>,
    buckets: btree_set::Iter<'b, usize>,
}

impl<'a, 'b> Iterator for PartialBucketIter<'a, 'b> {
    type Item = Bucket<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.buckets
            .next()
            .copied()
            .and_then(|index| self.table.bucket_at(index))
    }
}

fn bucket_row_iter(b: Bucket) -> RowHeaderIter<'_> {
    b.row_iter()
}

impl<'a> AsRowIter<'a> for MultiPKFilterSpec<'a> {
    type AsIter<'b> = MultiPKFilter<'a, 'b>
    where
        Self: 'b;

    fn as_row_iter(&self) -> Self::AsIter<'_> {
        MultiPKFilter {
            unfiltered_rows: PartialBucketIter {
                table: self.table,
                buckets: self.buckets.iter(),
            }
            .flat_map(bucket_row_iter),
            gate: &self.gate,
        }
    }
}

pub(super) struct MultiPKFilter<'a, 'b> {
    unfiltered_rows: iter::FlatMap<
        PartialBucketIter<'a, 'b>,
        RowHeaderIter<'a>,
        fn(Bucket<'a>) -> RowHeaderIter<'a>,
    >,
    gate: &'b ValueSet,
}

impl<'a, 'b> Iterator for MultiPKFilter<'a, 'b> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let row = self.unfiltered_rows.next()?;
            let pk = row.field_at(0).unwrap();
            if self.gate.contains(&pk) {
                return Some(row);
            }
        }
    }
}

pub(super) struct FilteredRowIter<'a> {
    inner: RowHeaderIter<'a>,
    gate: Value<FastContext>,
}

pub(super) struct FilteredRowIterSpec<'a> {
    bucket: Bucket<'a>,
    gate: Value<FastContext>,
}

impl<'a> FilteredRowIterSpec<'a> {
    /// Create a new instance
    pub fn new(bucket: Bucket<'a>, gate: Value<FastContext>) -> Self {
        Self { bucket, gate }
    }
}

impl<'a> AsRowIter<'a> for FilteredRowIterSpec<'a> {
    type AsIter<'b> = FilteredRowIter<'a>
    where
        Self: 'b;

    fn as_row_iter(&self) -> Self::AsIter<'_> {
        FilteredRowIter {
            inner: self.bucket.row_iter(),
            gate: self.gate.clone(),
        }
    }
}

impl<'a> Iterator for FilteredRowIter<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let gate = &self.gate;
        self.inner
            .by_ref()
            .find(|row| gate == &row.field_at(0).unwrap())
    }
}
