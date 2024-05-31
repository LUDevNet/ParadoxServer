use std::collections::HashMap;
use std::cmp::Ordering;
use std::fmt;
use std::fmt::Write;
use std::{borrow::Borrow, path::Path};

use rusqlite::{types::ValueRef, Connection, OpenFlags};

use graphql_parser::{
    parse_query,
    query::{Definition, Field, OperationDefinition, Selection},
};

use super::PercentDecoded;

#[derive(Debug)]
pub struct QueryError {
    pub error: String,
    pub message: String,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}: {}", self.error, self.message)
    }
}

impl std::error::Error for QueryError {}

impl From<graphql_parser::query::ParseError> for QueryError {
    fn from(value: graphql_parser::query::ParseError) -> Self {
        QueryError {
            error: String::from("GraphQL parsing error"),
            message: format!("{}", value),
        }
    }
}

impl From<rusqlite::Error> for QueryError {
    fn from(value: rusqlite::Error) -> Self {
        QueryError {
            error: String::from("rusqlite error"),
            message: format!("{}", value),
        }
    }
}

fn invalid_query(message: String) -> QueryError {
    QueryError {
        error: "invalid graphql query".to_string(),
        message,
    }
}

/// A relation between two SQL tables, a foreign key or its reverse.
#[derive(Clone, Debug, Default)]
pub struct TableRel {
    unique: bool,
    from_col: String,
    to_table: String,
    to_col: String,
}

pub type TableRels = HashMap<String, HashMap<String, TableRel>>;

#[derive(Debug)]
struct TableQuery {
    name: String,
    cols: Vec<Column>,
    constraints: Vec<String>,
    joins: Vec<Join>,
    // buffer for table_to_json
    rowid: i64,
    // buffer for table_to_json
    flushed_outputs: Vec<String>,
}

#[derive(Debug)]
struct Column {
    name: String,
    alias: Option<String>,
    // buffer for table_to_json
    value: Option<String>,
}

#[derive(Debug)]
struct Join {
    unique: bool,
    graphql_name: String,
    from_col: String,
    to_col: String,
    to_table: TableQuery,
}

/// Reads out foreign key and reverse relations from an SQLite DB.
pub fn read_out_table_rels(sqlite_path: &Path) -> Result<TableRels, rusqlite::Error> {
    let conn = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let mut table_rels: TableRels = HashMap::new();

    let mut tables_stmt = conn.prepare("select name from sqlite_master")?;
    let mut tables_rows = tables_stmt.query([])?;

    // for each table in the SQLite DB:
    while let Some(tables_row) = tables_rows.next()? {
        let from_table: String = tables_row.get(0)?;

        // temporary store for reverse relations
        // we don't insert them into table_rels immediately so we can insert shorthands when there is only one rel for a table
        let mut rev_rels: TableRels = HashMap::new();

        let mut fk_stmt =
            conn.prepare("select \"from\", \"table\", \"to\"  from pragma_foreign_key_list(?1)")?;
        let mut fk_rows = fk_stmt.query([&from_table])?;

        // for each foreign key in the table:
        while let Some(fk_row) = fk_rows.next()? {
            let from_col: String = fk_row.get(0)?;
            let to_table: String = fk_row.get(1)?;
            let to_col: String = fk_row.get(2)?;
            table_rels.entry(from_table.clone()).or_default().insert(
                from_col.clone(),
                TableRel {
                    unique: true,
                    from_col: from_col.clone(),
                    to_table: to_table.clone(),
                    to_col: to_col.clone(),
                },
            );

            // reverse relation

            let rev_col = format!("{}_{}", from_table, from_col);

            rev_rels.entry(to_table.clone()).or_default().insert(
                rev_col,
                TableRel {
                    unique: false,
                    from_col: to_col.clone(),
                    to_table: from_table.clone(),
                    to_col: from_col,
                },
            );
        }

        // merge rev rels into main rels
        for (table_name, rels) in rev_rels {
            let t_rels = table_rels.entry(table_name).or_default();
            if let Some(rel) = rels.values().next() {
                t_rels.insert(rel.to_table.clone(), rel.clone());
            }
            for (col_name, rel) in rels {
                t_rels.insert(col_name, rel);
            }
        }
    }

    Ok(table_rels)
}

/// Parses a GraphQl query, transforms it into equivalent SQL, runs it against the DB, and returns the output transformed to matching json.
pub(super) fn graphql(
    sqlite_path: &Path,
    table_rels: &TableRels,
    query: PercentDecoded,
) -> Result<String, QueryError> {
    let doc = parse_query::<String>(query.borrow())?;

    let def = match doc.definitions.len() {
        0 => {
            return Err(invalid_query("empty query".to_string()));
        }
        1 => match &doc.definitions[0] {
            Definition::Operation(op_def) => op_def,
            Definition::Fragment(_) => {
                return Err(invalid_query(
                    "TODO: fragment definition not supported".to_string(),
                ));
            }
        },
        n => {
            return Err(invalid_query(format!(
                "only 1 definition allowed, got: {n}"
            )));
        }
    };

    let selections = &match def {
        OperationDefinition::Query(query) => &query.selection_set,
        OperationDefinition::SelectionSet(sel) => sel,
        _ => {
            return Err(invalid_query(format!("Unsupported operation: {def}")));
        }
    }
    .items;

    let conn = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let mut kv = vec![];
    for selection in selections {
        match selection {
            Selection::FragmentSpread(_) => {
                return Err(invalid_query(
                    "TODO: Selection::FragmentSpread not supported".to_string(),
                ));
            }
            Selection::InlineFragment(_) => {
                return Err(invalid_query(
                    "TODO: Selection::InlineFragment not supported".to_string(),
                ));
            }
            Selection::Field(f) => {
                let mut table_query = field_to_table_query(table_rels, f)?;

                let query = table_query_to_sql(&table_query);

                let mut stmt = conn.prepare(&query)?;
                let mut rows = stmt.query([])?;
                let key = if let Some(alias) = &f.alias {
                    alias
                } else {
                    &f.name
                };
                kv.push(format!(
                    "\"{}\":{}",
                    key,
                    table_to_json(&mut table_query, &mut rows)?
                ));
            }
        }
    }
    Ok(format!("{{{}}}", kv.join(",")))
}

/// Recursively parses a GraphQL field into an abstract TableQuery.
fn field_to_table_query(
    table_rels: &TableRels,
    field: &Field<String>,
) -> Result<TableQuery, QueryError> {
    field_to_table_query_inner(table_rels, field, field.name.as_ref())
}

fn field_to_table_query_inner(
    table_rels: &TableRels,
    field: &Field<String>,
    table_name: &str,
) -> Result<TableQuery, QueryError> {
    let mut table_query = TableQuery {
        name: table_name.to_string(),
        cols: vec![],
        constraints: vec![],
        rowid: 0,
        joins: vec![],
        flushed_outputs: vec![],
    };

    for (key, value) in &field.arguments {
        table_query
            .constraints
            .push(format!("{} == {}", key, value));
    }

    let this_table_rels = if let Some(rels) = table_rels.get(&table_query.name) {
        rels
    } else {
        return Err(QueryError {
            error: format!("invalid table name {}", table_query.name),
            message: "table does not exist".to_string(),
        });
    };

    for selection in &field.selection_set.items {
        match selection {
            Selection::Field(f) => {
                if f.selection_set.items.is_empty() {
                    // no curly braces, this is a normal column
                    // if it ends in _loc, query the column with the appropriate localization
                    table_query.cols.push(if f.name.ends_with("_loc") {
                        let mut localized_name = f.name.clone();
                        localized_name.truncate(localized_name.len() - "loc".len());
                        localized_name.push_str("en_US"); // todo: proper localization support
                        Column {
                            name: localized_name,
                            alias: Some(f.name.clone()),
                            value: None,
                        }
                    } else {
                        // normal column
                        Column {
                            name: f.name.clone(),
                            alias: f.alias.clone(),
                            value: None,
                        }
                    });
                } else {
                    // curly braces, this requires the field to be a valid relation
                    let rel = if let Some(rel) = this_table_rels.get(&f.name) {
                        rel
                    } else {
                        return Err(QueryError {
                            error: format!("field {} is not a foreign key", &f.name),
                            message: "field has items but is not a FK according to the DB"
                                .to_string(),
                        });
                    };

                    // recurse with the fields in curly braces
                    let tq = field_to_table_query_inner(table_rels, f, rel.to_table.as_ref())?;

                    // link the parent query to the child query
                    table_query.joins.push(Join {
                        unique: rel.unique,
                        graphql_name: f.name.clone(),
                        from_col: rel.from_col.clone(),
                        to_col: rel.to_col.clone(),
                        to_table: tq,
                    });
                }
            }
            _ => {
                return Err(invalid_query(
                    "selection set in query should only contain Fields".to_string(),
                ));
            }
        }
    }

    Ok(table_query)
}

/// Generates equivalent SQL from a parsed TableQuery.
fn table_query_to_sql(table_query: &TableQuery) -> String {
    let mut cols = vec![];
    let mut tables = vec![];
    let mut constraints = vec![];

    tables.push(format!("{} as t0", table_query.name));

    table_query_to_sql_inner(table_query, &mut cols, &mut tables, &mut constraints);

    let mut query = format!("select {} from {}", cols.join(", "), tables.join(" "));
    if !constraints.is_empty() {
        write!(query, " where {}", constraints.join(" and ")).unwrap();
    }
    write!(
        query,
        " order by {}",
        (0..tables.len())
            .map(|x| format!("t{}.rowid", x))
            .collect::<Vec<String>>()
            .join(", ")
    )
    .unwrap();
    query
}

fn table_query_to_sql_inner(
    table_query: &TableQuery,
    cols: &mut Vec<String>,
    tables: &mut Vec<String>,
    constraints: &mut Vec<String>,
) {
    let table_n = tables.len() - 1;
    let mut table_n2 = table_n;

    cols.push(format!("t{}.rowid", table_n));

    for col in &table_query.cols {
        cols.push(format!("t{}.{}", table_n, col.name));
    }

    for constraint in &table_query.constraints {
        constraints.push(format!("t{}.{}", table_n, constraint));
    }

    for join in &table_query.joins {
        tables.push(format!(
            "left join {} as t{} on t{}.{} = t{}.{}",
            join.to_table.name,
            table_n2 + 1,
            table_n,
            join.from_col,
            table_n2 + 1,
            join.to_col
        ));
        table_query_to_sql_inner(&join.to_table, cols, tables, constraints);
        table_n2 = tables.len() - 1;
    }
}

/// Formats a sqlite value as json.
fn valueref_to_json(valueref: &ValueRef) -> Result<String, rusqlite::Error> {
    let mut str = String::new();
    match valueref {
        ValueRef::Null => str.push_str("null"),
        ValueRef::Integer(x) => str.push_str(&x.to_string()),
        ValueRef::Real(x) => str.push_str(&x.to_string()),
        ValueRef::Text(x) | ValueRef::Blob(x) => {
            str.push('"');
            str.push_str(
                &std::str::from_utf8(x)
                    .map_err(rusqlite::Error::Utf8Error)?
                    .replace('\\', "\\\\")
                    .replace('"', "\\\""),
            );
            str.push('"');
        }
    }
    Ok(str)
}

/// Given an SQLite query result `rows`, use the structure info from `table_query` to transform it to hierarchical JSON.
fn table_to_json(
    table_query: &mut TableQuery,
    rows: &mut rusqlite::Rows,
) -> Result<String, rusqlite::Error> {
    while let Some(row) = rows.next()? {
        let mut icol = 0;
        // read in the data into the right buffers...
        table_to_json_inner(table_query, &mut icol, false, row)?;
    }
    // ...and convert it to json
    if table_query.rowid > 0 {
        let out = flush_table_data(table_query);
        table_query.flushed_outputs.push(out);
    }
    Ok(format!("[{}]", table_query.flushed_outputs.join(",")))
}

/// Given a single SQLite query result row, recursively stores the data in the appropriate TableQuery buffers.
fn table_to_json_inner(
    table_query: &mut TableQuery,
    // Index of current column to read from.
    icol: &mut usize,
    // Whether to skip reading data, but still advance the column index for all subqueries.
    mut skip: bool,
    row: &rusqlite::Row,
) -> Result<(), rusqlite::Error> {
    if !skip {
        let rowid_col = row.get_ref(*icol)?;
        *icol += 1;

        if let rusqlite::types::ValueRef::Integer(rowid) = rowid_col {
            // since SQL joins duplicate values, we use the sorted rowid to deduplicate
            match rowid.cmp(&table_query.rowid) {
                Ordering::Greater => {
                    // new row, read in data
                    if table_query.rowid > 0 {
                        // we read a row before, whose subbuffers need to be flushed out
                        let out = flush_table_data(table_query);
                        table_query.flushed_outputs.push(out);
                    }
                    // keep track of this rowid for the next row'scheck
                    table_query.rowid = rowid;

                    for col in &mut table_query.cols {
                        col.value = Some(valueref_to_json(&row.get_ref(*icol)?)?);
                        *icol += 1;
                    }
                }
                Ordering::Equal => {
                    // already encountered this entry in a previous join, skip this table but not subtables, which might have new subentries
                    *icol += table_query.cols.len();
                }
                Ordering::Less => {
                    // already encountered this entry in a previous join, skip this and all subtables
                    skip = true;
                    *icol += table_query.cols.len();
                }
            }
        } else {
            // left join is null, skip
            if table_query.rowid > 0 {
                // ...but null joins still need to flush out old rows
                let out = flush_table_data(table_query);
                table_query.flushed_outputs.push(out);
            }
            *icol += table_query.cols.len();
        }
    } else {
        *icol += table_query.cols.len() + 1;
    }

    // recurse for each subquery
    for join in &mut table_query.joins {
        table_to_json_inner(&mut join.to_table, icol, skip, row)?;
    }

    Ok(())
}

/// Serializes this query's buffers to JSON and then clears them, returns the JSON.
fn flush_table_data(table_query: &mut TableQuery) -> String {
    let mut kv = vec![];
    for col in &table_query.cols {
        let key = if let Some(alias) = &col.alias {
            alias
        } else {
            &col.name
        };
        kv.push(format!("\"{}\":{}", key, col.value.as_ref().unwrap()));
    }

    for join in &mut table_query.joins {
        if join.unique {
            let out = if join.to_table.rowid > 0 {
                flush_table_data(&mut join.to_table)
            } else {
                String::from("null")
            };
            kv.push(format!("\"{}\":{}", join.graphql_name, out));
        } else {
            if join.to_table.rowid > 0 {
                let out = flush_table_data(&mut join.to_table);
                join.to_table.flushed_outputs.push(out);
            }
            kv.push(format!(
                "\"{}\":[{}]",
                join.graphql_name,
                join.to_table.flushed_outputs.join(",")
            ));
        }
        join.to_table.flushed_outputs.clear();
    }
    table_query.rowid = 0;
    format!("{{{}}}", kv.join(","))
}
