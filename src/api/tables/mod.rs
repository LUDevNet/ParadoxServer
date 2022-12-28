use std::{
    borrow::Cow,
    fmt,
    num::{ParseFloatError, ParseIntError},
};

use assembly_core::buffer::CastError;
use assembly_fdb::{
    mem::Database,
    value::{Context, Value, ValueType},
    FdbHash,
};
use http::StatusCode;
use hyper::body::Bytes;
use latin1str::Latin1String;
use serde::Serialize;

use super::{Accept, ApiResult};

mod query;
mod util;

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
        util::RowIter::new(t, to_cols)
    }))
}

pub(super) async fn table_all_query<'a, B>(
    db: Database<'a>,
    accept: Accept,
    name: &str,
    body: B,
) -> ApiResult
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: fmt::Display,
{
    let tables = db.tables()?;
    let Some(table) = tables.by_name(name).transpose()? else {
        return Ok(super::reply_404());
    };

    let pk_col = table
        .column_at(0)
        .expect("Tables must have at least 1 column");
    let bytes = match hyper::body::to_bytes(body).await {
        Ok(b) => b,
        Err(e) => return super::reply_400(accept, "Failed to aggregate query body", e),
    };

    let ty = pk_col.value_type();
    let _req = match query::TableQuery::new(ty, &bytes) {
        Ok(v) => v,
        Err(e) => return super::reply_400(accept, "Failed to parse query body", e),
    };

    let names = table.column_iter().map(|c| c.name()).collect::<Vec<_>>();
    let to_cols = util::PartialColValIterSpec::new(names, &_req.columns);

    super::reply(
        accept,
        &util::RowIter::<'a, _, _>::new(util::MultiPKFilterSpec::new(table, _req.pks), to_cols),
        StatusCode::OK,
    )
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

    Ok(Some(util::RowIter::new(
        util::FilteredRowIterSpec::new(bucket, gate),
        table.column_iter().map(|c| c.name()).collect::<Vec<_>>(),
    )))
}
