use std::borrow::Cow;

use assembly_core::buffer::CastError;
use assembly_fdb::{
    common::ValueType,
    mem::{Column, Database, Row, RowHeaderIter},
    query::{self, PrimaryKeyFilter},
};
use linked_hash_map::LinkedHashMap;
use serde::{ser::SerializeSeq, Serialize};

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

pub(super) fn table_all_json<'a>(
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

struct FilteredRowIter<'a> {
    inner: RowHeaderIter<'a>,
    gate: PrimaryKeyFilter,
}

impl<'a> Iterator for FilteredRowIter<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.inner.by_ref() {
            if self.gate.filter(&next.field_at(0).unwrap()) {
                return Some(next);
            }
        }
        None
    }
}

pub(super) fn table_key_json<'a>(
    db: Database<'a>,
    name: &str,
    key: String,
) -> Result<Option<impl Serialize + 'a>, CastError> {
    let tables = db.tables()?;
    let table = match tables.by_name(name) {
        Some(t) => t?,
        None => return Ok(None),
    };

    let index_field = table.column_at(0).unwrap();
    let index_field_type = index_field.value_type();

    let pk_filter = match query::pk_filter(key, index_field_type) {
        Ok(f) => f,
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
