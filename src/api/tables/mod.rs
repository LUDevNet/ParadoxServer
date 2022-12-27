use std::{
    borrow::Cow,
    num::{ParseFloatError, ParseIntError},
};

use assembly_core::buffer::CastError;
use assembly_fdb::{
    mem::{Column, Database, Row, RowHeaderIter},
    value::{Context, Value, ValueType},
    FdbHash,
};
use hyper::body::Bytes;
use latin1str::Latin1String;
use linked_hash_map::LinkedHashMap;
use serde::{ser::SerializeSeq, Serialize};

use super::ApiError;

mod query;

struct RowIter<'a, C, R, FR>
where
    C: Iterator<Item = Column<'a>> + Clone,
    R: Iterator<Item = Row<'a>>,
    FR: Fn() -> R,
{
    cols: C,
    to_rows: FR,
}

impl<'a, C, R, FR> Serialize for RowIter<'a, C, R, FR>
where
    C: Iterator<Item = Column<'a>> + Clone,
    R: Iterator<Item = Row<'a>>,
    FR: Fn() -> R,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_seq(None)?;
        for r in (self.to_rows)() {
            let mut row = LinkedHashMap::new();
            let mut fields = r.field_iter();
            for col in self.cols.clone() {
                let col_name = col.name();
                let field = fields.next().unwrap();
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
        RowIter {
            cols: t.column_iter(),
            // FIXME: reintroduce cap
            to_rows: move || t.row_iter(), /*.take(100) */
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
{
    let tables = db.tables()?;
    let table = tables.by_name(name).transpose()?;

    Ok(match table {
        Some(t) => {
            let pk_col = t.column_at(0).expect("Tables must have at least 1 column");
            let body_data = match body.data().await {
                Some(Ok(b)) => b,
                Some(Err(_e)) => return Ok(None), // FIXME: 40X
                None => return Ok(None),
            };
            let ty = pk_col.value_type();
            let _req = match query::TableQuery::new(ty, body_data.as_ref()) {
                Ok(v) => v,
                Err(_e) => return Ok(None), // FIXME: 40X
            };

            Some(RowIter {
                cols: t.column_iter(),
                // FIXME: reintroduce cap
                to_rows: move || t.row_iter(), /*.take(100) */
            })
        }
        None => None,
    })
}

struct FilteredRowIter<'a> {
    inner: RowHeaderIter<'a>,
    gate: Value<FastContext>,
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

    let pk_filter = match FastContext::parse_as(key, index_field_type) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let bucket_index = pk_filter.hash() as usize % table.bucket_count();
    let bucket = table.bucket_at(bucket_index).unwrap();

    let cols = table.column_iter();
    let to_rows = move || FilteredRowIter {
        inner: bucket.row_iter(),
        gate: pk_filter.clone(),
    };

    Ok(Some(RowIter { cols, to_rows }))
}
