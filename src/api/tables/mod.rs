use std::{
    borrow::Cow,
    collections::{btree_set, BTreeSet},
    fmt,
    iter::{self, Cloned, Copied, Zip},
    marker::PhantomData,
    num::{ParseFloatError, ParseIntError},
    slice::Iter,
};

use assembly_core::buffer::CastError;
use assembly_fdb::{
    mem::{iter::TableRowIter, Bucket, Database, FieldIter, MemContext, Row, RowHeaderIter, Table},
    value::{Context, Value, ValueType},
    FdbHash,
};
use hyper::body::Bytes;
use latin1str::Latin1String;
use linked_hash_map::LinkedHashMap;
use serde::{ser::SerializeSeq, Serialize};

use self::query::ValueSet;

use super::ApiError;

mod query;

trait AsRowIter<'a> {
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

trait AsColValIter<'a> {
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

type ColValIter<'a, 'b> = Zip<Cloned<Iter<'b, Cow<'a, str>>>, FieldIter<'a>>;
#[allow(dead_code)] // false positive
type ColValPair<'a> = (Cow<'a, str>, Value<MemContext<'a>>);

struct PartialColValIterSpec<'a> {
    indices: Vec<usize>,
    names: Vec<Cow<'a, str>>,
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
struct PartialColValIter<'a, 'b> {
    index: Copied<Iter<'b, usize>>,
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

struct RowIter<'a, FR, FC>
where
    FR: AsRowIter<'a>,
    FC: AsColValIter<'a>,
{
    to_rows: FR,
    to_cols: FC,
    _p: PhantomData<&'a ()>,
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
        let mut s = serializer.serialize_seq(None)?;
        for r in self.to_rows.as_row_iter() {
            let mut row = LinkedHashMap::new();
            for (col_name, field) in self.to_cols.as_cv_iter(r) {
                row.insert(col_name, field);
            }
            s.serialize_element(&row)?;
        }
        s.end()
    }
}

#[derive(Serialize)]
pub(super) struct TableDef<'a> {
    name: Cow<'a, str>,
    columns: Vec<TableCol<'a>>,
}

#[derive(Serialize)]
struct TableCol<'a> {
    name: Cow<'a, str>,
    data_type: ValueType,
}

pub(super) fn tables_json(db: Database) -> Result<Vec<Cow<'_, str>>, CastError> {
    let tables = db.tables()?;
    let mut list = Vec::with_capacity(tables.len());
    for table in tables.iter() {
        list.push(table?.name());
    }
    Ok(list)
}

pub(super) fn table_def_json<'a>(
    db: Database<'a>,
    name: &str,
) -> Result<Option<TableDef<'a>>, CastError> {
    let tables = db.tables()?;
    if let Some(table) = tables.by_name(name) {
        let table = table?;
        let name = table.name();
        let columns: Vec<_> = table
            .column_iter()
            .map(|col| TableCol {
                name: col.name(),
                data_type: col.value_type(),
            })
            .collect();
        Ok(Some(TableDef { name, columns }))
    } else {
        Ok(None)
    }
}

pub(super) fn table_all_get<'a>(
    db: Database<'a>,
    name: &str,
) -> Result<Option<impl Serialize + 'a>, CastError> {
    let tables = db.tables()?;
    let table = tables.by_name(name).transpose()?;

    Ok(table.map(|t| {
        let to_cols: Vec<_> = t.column_iter().map(|col| col.name()).collect();
        RowIter {
            to_cols,
            // FIXME: reintroduce cap
            to_rows: t,
            _p: PhantomData,
        }
    }))
}

pub(super) async fn table_all_query<'a, B>(
    db: Database<'a>,
    name: &str,
    mut body: B,
) -> Result<Option<impl Serialize + 'a>, ApiError>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: fmt::Display,
{
    let tables = db.tables()?;
    let table = tables.by_name(name).transpose()?;

    Ok(match table {
        Some(t) => {
            let pk_col = t.column_at(0).expect("Tables must have at least 1 column");
            let body_data = match body.data().await {
                Some(Ok(b)) => b,
                Some(Err(e)) => {
                    tracing::warn!("{}", e);
                    return Ok(None); // FIXME: 40X
                }
                None => {
                    tracing::warn!("Missing Body Bytes");
                    return Ok(None);
                }
            };
            let ty = pk_col.value_type();
            let _req = match query::TableQuery::new(ty, body_data.as_ref()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("{}", e);
                    return Ok(None); // FIXME: 40X
                }
            };
            let buckets = _req.bucket_set(t.bucket_count());
            let names = t.column_iter().map(|c| c.name()).collect::<Vec<_>>();

            Some(RowIter {
                to_cols: PartialColValIterSpec {
                    indices: names
                        .iter()
                        .map(Cow::as_ref)
                        .enumerate()
                        .filter_map(|(i, name)| match _req.columns.contains(name) {
                            true => Some(i),
                            false => None,
                        })
                        .collect::<Vec<_>>(),
                    names,
                },
                to_rows: MultiPKFilterSpec {
                    table: t,
                    buckets,
                    gate: _req.pks,
                },
                _p: PhantomData,
            })
        }
        None => None,
    })
}

struct MultiPKFilterSpec<'a> {
    table: Table<'a>,
    buckets: BTreeSet<usize>,
    gate: ValueSet,
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

struct MultiPKFilter<'a, 'b> {
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

struct FilteredRowIter<'a> {
    inner: RowHeaderIter<'a>,
    gate: Value<FastContext>,
}

struct FilteredRowIterSpec<'a> {
    bucket: Bucket<'a>,
    gate: Value<FastContext>,
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

struct FastContext;

impl Context for FastContext {
    type I64 = i64;
    type String = Latin1String;
    type XML = Latin1String;
}

struct ParseError;

impl From<ParseIntError> for ParseError {
    fn from(_: ParseIntError) -> Self {
        Self
    }
}

impl From<ParseFloatError> for ParseError {
    fn from(_: ParseFloatError) -> Self {
        Self
    }
}

impl FastContext {
    pub fn parse_as(v: &str, ty: ValueType) -> Result<Value<FastContext>, ParseError> {
        Ok(match ty {
            ValueType::Nothing => Value::Nothing,
            ValueType::Integer => Value::Integer(v.parse()?),
            ValueType::Float => Value::Float(v.parse()?),
            ValueType::Text => Value::Text(Latin1String::encode(v).into_owned()),
            ValueType::Boolean => match v {
                "true" | "1" | "TRUE" => Value::Boolean(true),
                "false" | "0" | "FALSE" => Value::Boolean(false),
                _ => return Err(ParseError),
            },
            ValueType::BigInt => Value::BigInt(v.parse()?),
            ValueType::VarChar => return Err(ParseError),
        })
    }
}

pub(super) fn table_key_json<'a>(
    db: Database<'a>,
    name: &str,
    key: &str,
) -> Result<Option<impl Serialize + 'a>, CastError> {
    let tables = db.tables()?;
    let table = match tables.by_name(name) {
        Some(t) => t?,
        None => return Ok(None),
    };

    let index_field = table.column_at(0).unwrap();
    let index_field_type = index_field.value_type();

    let gate = match FastContext::parse_as(key, index_field_type) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let bucket_index = gate.hash() as usize % table.bucket_count();
    let bucket = table.bucket_at(bucket_index).unwrap();

    Ok(Some(RowIter {
        to_cols: table.column_iter().map(|c| c.name()).collect::<Vec<_>>(),
        to_rows: FilteredRowIterSpec { bucket, gate },
        _p: PhantomData,
    }))
}
