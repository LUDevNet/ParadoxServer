use std::{borrow::Borrow, path::Path};

use rusqlite::{types::ValueRef, Connection, OpenFlags};

use super::PercentDecoded;

fn fmt_valueref(str: &mut String, valueref: &ValueRef) -> Result<(), rusqlite::Error> {
    match valueref {
        ValueRef::Null => str.push_str("null"),
        ValueRef::Integer(x) => str.push_str(&x.to_string()),
        ValueRef::Real(x) => str.push_str(&x.to_string()),
        ValueRef::Text(x) | ValueRef::Blob(x) => {
            str.push('"');
            str.push_str(
                &std::str::from_utf8(x)
                    .map_err(rusqlite::Error::Utf8Error)?
                    .replace('"', "\"\""),
            );
            str.push('"');
        }
    }
    Ok(())
}

pub(super) fn query(
    sqlite_path: &Path,
    query: PercentDecoded,
) -> Result<String, rusqlite::Error> {
    dbg!(&query);
    let conn = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(query.borrow())?;

    let cols = stmt.column_count();
    let mut response = String::new();
    response.push_str(&stmt.column_names().join(","));
    response.push('\n');

    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        for i in 0..(cols - 1) {
            fmt_valueref(&mut response, &row.get_ref(i)?)?;
            response.push(',');
        }
        fmt_valueref(&mut response, &row.get_ref(cols - 1)?)?;
        response.push('\n');
    }
    Ok(response)
}
