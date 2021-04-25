use std::{borrow::Cow, convert::Infallible, error::Error, sync::Arc};

use assembly_core::buffer::CastError;
use assembly_data::{
    fdb::{common::ValueType, mem::Database, query},
    xml::localization::LocaleNode,
};
use linked_hash_map::LinkedHashMap;
use serde::Serialize;
use warp::{
    path::Tail,
    reply::{Json, WithStatus},
    Filter, Rejection, Reply,
};

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

fn map_res<E: Error>(v: Result<Json, E>) -> WithStatus<Json> {
    match v {
        Ok(res) => wrap_200(res),
        Err(e) => wrap_500(warp::reply::json(&e.to_string())),
    }
}

fn map_opt_res<E: Error>(v: Result<Option<Json>, E>) -> WithStatus<Json> {
    match v {
        Ok(Some(res)) => wrap_200(res),
        Ok(None) => wrap_404(warp::reply::json(&())),
        Err(e) => wrap_500(warp::reply::json(&e.to_string())),
    }
}

fn map_opt(v: Option<Json>) -> WithStatus<Json> {
    match v {
        Some(res) => wrap_200(res),
        None => wrap_404(warp::reply::json(&())),
    }
}

fn tables_api(db: Database) -> Result<Json, CastError> {
    let tables = db.tables()?;
    let mut list = Vec::with_capacity(tables.len());
    for table in tables.iter() {
        let table = table?;
        let name = table.name();
        list.push(name);
    }
    Ok(warp::reply::json(&list))
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

fn table_key_api(db: Database<'_>, name: String, key: String) -> Result<Option<Json>, CastError> {
    let tables = db.tables()?;
    if let Some(table) = tables.by_name(&name) {
        let table = table?;
        let mut cols = table.column_iter();
        let index_field = cols.next().unwrap();
        let index_field_type = index_field.value_type();
        let index_name = index_field.name();

        match query::pk_filter(key, index_field_type) {
            Ok(pk_filter) => {
                let bucket_index = pk_filter.hash() as usize % table.bucket_count();
                let bucket = table.bucket_at(bucket_index).unwrap();
                let mut rows = Vec::new();

                for row in bucket.row_iter() {
                    let mut field_iter = row.field_iter();
                    let index_mem = field_iter.next().unwrap();
                    if pk_filter.filter(&index_mem) {
                        let mut row = LinkedHashMap::new();
                        row.insert(index_name.clone(), index_mem);
                        // add the remaining fields
                        #[allow(clippy::clone_on_copy)]
                        for col in cols.clone() {
                            let col_name = col.name();
                            let field = field_iter.next().unwrap();
                            row.insert(col_name, field);
                        }
                        rows.push(row);
                    }
                }
                Ok(Some(warp::reply::json(&rows)))
            }
            Err(_e) => Ok(None),
        }
    } else {
        Ok(None)
    }

    //let lname = Latin1String::encode(&name);
    //let db_table = db_table.as_bytes_handle();

    //wrap_200(table_index(db_table, lname.as_ref(), key))
}

fn wrap_404<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND)
}

pub fn wrap_200<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::OK)
}

pub fn wrap_500<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::INTERNAL_SERVER_ERROR)
}

fn make_api_catch_all() -> impl Filter<Extract = (WithStatus<Json>,), Error = Infallible> + Clone {
    warp::any().map(|| warp::reply::json(&404)).map(wrap_404)
}

fn make_api_tables(
    db: Database<'_>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Send + '_
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
    // The `/tables/:name/:key` endpoint
    let table_get = table
        .and(warp::path::param())
        .map(table_key_api)
        .map(map_opt_res);

    tables.or(table_def).unify().or(table_get).unify()
}

fn db_filter<'db>(
    db: Database<'db>,
) -> impl Filter<Extract = (Database,), Error = Infallible> + Clone + 'db {
    warp::any().map(move || db)
}

#[derive(Debug, Serialize)]
struct LocalePod<'a> {
    value: Option<&'a str>,
    int_keys: Vec<u32>,
    str_keys: Vec<&'a str>,
}

pub fn make_api(
    db: Database,
    lr: Arc<LocaleNode>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Infallible> + Clone + '_
//where
//    B: AsRef<[u8]> + Send + Sync + 'db,
{
    // v0
    let v0_base = warp::path("v0");
    let v0_tables = warp::path("tables").and(make_api_tables(db));
    let v0_locale = warp::path("locale")
        .and(warp::path::tail())
        .map(move |p: Tail| {
            let path = p.as_str().trim_end_matches('/');
            let mut node = lr.as_ref();
            if !path.is_empty() {
                // Skip loop for root node
                for seg in path.split('/') {
                    if let Some(new) = {
                        if let Ok(num) = seg.parse::<u32>() {
                            node.int_children.get(&num)
                        } else {
                            node.str_children.get(seg)
                        }
                    } {
                        node = new;
                    } else {
                        return None;
                    }
                }
            }
            Some(warp::reply::json(&LocalePod {
                value: node.value.as_deref(),
                int_keys: node.int_children.keys().cloned().collect(),
                str_keys: node.str_children.keys().map(|s| s.as_ref()).collect(),
            }))
        })
        .map(map_opt);
    let v0 = v0_base.and(v0_tables.or(v0_locale).unify());

    // v1
    let dbf = db_filter(db);
    let v1_base = warp::path("v1");
    let v1_tables_base = dbf.and(warp::path("tables"));
    let v1_tables = v1_tables_base
        .and(warp::path::end())
        .map(tables_api)
        .map(map_res);
    let v1 = v1_base.and(v1_tables);

    // catch all
    let catch_all = make_api_catch_all();

    v0.or(v1).unify().or(catch_all).unify()
}
