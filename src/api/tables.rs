use std::borrow::Cow;

use assembly_core::buffer::CastError;
use assembly_fdb::{
    common::ValueType,
    mem::{Column, Database, Row},
    query,
};
use linked_hash_map::LinkedHashMap;
use serde::{ser::SerializeSeq, Serialize};
use warp::{
    filters::BoxedFilter,
    reply::{Json, WithStatus},
    Filter,
};

use super::{db_filter, map_opt_res, map_res};

/*fn table_index(db_table: Handle<'_, FDBHeader>, lname: &Latin1Str, key: String) -> Json {
    let table = db_table.into_table_by_name(lname).unwrap();

    if let Some(table) = table.transpose() {
        let table_def = table.into_definition().unwrap();
        let table_data = table.into_data().unwrap();

        let mut cols = table_def.column_header_list().unwrap();
        let index_col = cols.next().unwrap();
        let index_type = ValueType::try_from(index_col.raw().column_data_type).unwrap();
        let index_name = index_col.column_name().unwrap().raw().decode();

        if let Ok(pk_filter) = query::pk_filter(key, index_type) {
            let bucket_index = pk_filter.hash() % table_data.raw().buckets.count;
            let mut buckets = table_data.bucket_header_list().unwrap();
            let bucket = buckets.nth(bucket_index as usize).unwrap();

            let mut rows = Vec::new();
            for row_header in bucket.row_header_iter() {
                let row_header = row_header.unwrap();

                let mut field_iter = row_header.field_data_list().unwrap();
                let index_field = field_iter.next().unwrap();
                let index_value = index_field.try_get_value().unwrap();
                let index_mem = mem::Field::try_from(index_value).unwrap();

                if !pk_filter.filter(&index_mem) {
                    continue;
                }

                let mut row = LinkedHashMap::new();
                row.insert(index_name.clone(), index_mem);
                // add the remaining fields
                #[allow(clippy::clone_on_copy)]
                for col in cols.clone() {
                    let col_name = col.column_name().unwrap().raw().decode();
                    let field = field_iter.next().unwrap();
                    let value = field.try_get_value().unwrap();
                    let mem_val = mem::Field::try_from(value).unwrap();
                    row.insert(col_name, mem_val);
                }
                rows.push(row);
            }

            return warp::reply::json(&rows);
        }
    }
    warp::reply::json(&())
}*/

/*fn tables_api<B: AsRef<[u8]>>(db: ArcHandle<B, FDBHeader>) -> WithStatus<Json> {
    let db = db.as_bytes_handle();

    let mut list = Vec::with_capacity(db.raw().tables.count as usize);
    let header_list = db.table_header_list().unwrap();
    for tbl in header_list {
        let def = tbl.into_definition().unwrap();
        let name = *def.table_name().unwrap().raw();
        list.push(name);
    }
    let reply = warp::reply::json(&list);
    warp::reply::with_status(reply, warp::http::StatusCode::OK)
}*/

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
struct TableDef<'a> {
    name: Cow<'a, str>,
    columns: Vec<TableCol<'a>>,
}

#[derive(Serialize)]
struct TableCol<'a> {
    name: Cow<'a, str>,
    data_type: ValueType,
}

pub(super) fn tables_api(db: Database) -> Result<Json, CastError> {
    let tables = db.tables()?;
    let mut list = Vec::with_capacity(tables.len());
    for table in tables.iter() {
        let table = table?;
        let name = table.name();
        list.push(name);
    }
    Ok(warp::reply::json(&list))
}

fn table_def_api(db: Database<'_>, name: String) -> Result<Option<Json>, CastError> {
    let tables = db.tables()?;
    if let Some(table) = tables.by_name(&name) {
        let table = table?;
        let name = table.name();
        let columns: Vec<_> = table
            .column_iter()
            .map(|col| TableCol {
                name: col.name(),
                data_type: col.value_type(),
            })
            .collect();
        Ok(Some(warp::reply::json(&TableDef { name, columns })))
    } else {
        Ok(None)
    }
}

fn table_all_api(db: Database<'_>, name: String) -> Result<Option<Json>, CastError> {
    let tables = db.tables()?;
    let table = match tables.by_name(&name) {
        Some(t) => t?,
        None => return Ok(None),
    };

    let cols = table.column_iter();
    let to_rows = || table.row_iter().take(100);

    Ok(Some(warp::reply::json(&RowIter { cols, to_rows })))
}

fn table_key_api(db: Database<'_>, name: String, key: String) -> Result<Option<Json>, CastError> {
    let tables = db.tables()?;
    let table = match tables.by_name(&name) {
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

    let filter = &pk_filter;
    let cols = table.column_iter();
    let to_rows = || {
        return bucket
            .row_iter()
            .filter(move |r| filter.filter(&r.field_at(0).unwrap()));
    };

    Ok(Some(warp::reply::json(&RowIter { cols, to_rows })))
}

pub(super) fn make_api_tables(db: Database<'static>) -> BoxedFilter<(WithStatus<Json>,)>
//where
    //H: Filter<Extract = (ArcHandle<B, FDBHeader>,), Error = Infallible> + Clone + Send,
{
    let dbf = db_filter(db);
    //let tables_base = hnd_state;

    // The `/tables` endpoint
    let tables = dbf
        .clone()
        .and(warp::path::end())
        .map(tables_api)
        .map(map_res);
    let table = dbf.and(warp::path::param());

    // The `/tables/:name/def` endpoint
    let table_def = table
        .clone()
        .and(warp::path("def"))
        .map(table_def_api)
        .map(map_opt_res);
    // The `/tables/:name/all` endpoint
    let table_all = table
        .clone()
        .and(warp::path("all"))
        .map(table_all_api)
        .map(map_opt_res);
    // The `/tables/:name/:key` endpoint
    let table_get = table
        .and(warp::path::param())
        .map(table_key_api)
        .map(map_opt_res);

    tables
        .or(table_def)
        .unify()
        .or(table_all)
        .unify()
        .or(table_get)
        .unify()
        .boxed()
}
