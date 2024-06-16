use std::{collections::HashMap, fmt::Write, path::PathBuf, time::Instant};

use argh::FromArgs;
use color_eyre::eyre::WrapErr;
use rusqlite::Connection;

#[derive(FromArgs)]
/// Outputs a GraphQL schema file based on the schema of the SQLite database
struct Options {
    /// input SQLite file
    #[argh(positional)]
    src: PathBuf,
    /// the GraphQL schema output file
    #[argh(positional)]
    dest: PathBuf,
}

fn to_gql_type(sql_type: &str) -> String {
    String::from(match sql_type {
        "INTEGER" | "INT32" | "INT64" | "INT_BOOL" => "Int",
        "TEXT4" | "TEXT_XML" | "BLOB_NONE" => "String",
        "REAL" => "Float",
        _ => todo!(),
    })
}

fn try_export_db(conn: &mut Connection) -> color_eyre::Result<String> {
    let mut out = String::from("schema {\n\tquery: Query\n}\n\n");

    let mut tables_stmt = conn.prepare("select m.name, c.name, c.type, c.\"notnull\", fk.\"table\" from sqlite_master m join pragma_table_info(m.name) c left join pragma_foreign_key_list(m.name) fk on c.name == fk.\"from\"")?;
    let mut tables_rows = tables_stmt.query([])?;

    let mut table_rels: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    let mut rev_rels: HashMap<String, Vec<String>> = HashMap::new();
    let mut last_table = String::new();

    while let Some(tables_row) = tables_rows.next()? {
        let table_name: String = tables_row.get(0)?;
        let col_name: String = tables_row.get(1)?;
        let col_type: String = tables_row.get(2)?;
        let col_type = to_gql_type(&col_type);
        let not_null: bool = tables_row.get(3)?;
        let fk_table: Option<String> = tables_row.get(4)?;

        if table_name != last_table {
            for (other_table, rels) in rev_rels {
                let t_rels = table_rels.entry(other_table).or_default();
                if rels.len() == 1 {
                    if let Some(col) = rels.iter().next() {
                        t_rels.push((
                            format!(
                                "{}: {}",
                                &col[..col.find("_").unwrap()],
                                &col[col.find(":").unwrap() + 2..]
                            ),
                            true,
                        ));
                    }
                }
                for col in rels {
                    t_rels.push((col, true));
                }
            }
            rev_rels = HashMap::new();
            last_table = table_name.clone();
        }

        let tbl = table_rels.entry(table_name.clone()).or_default();

        let mut col_ty = String::new();
        write!(
            col_ty,
            "{}",
            if let Some(ref x) = fk_table {
                &x
            } else {
                &col_type
            }
        )?;
        if col_name.ends_with("_en_US") {
            let mut loc_name = col_name.clone();
            loc_name.truncate(loc_name.len() - "en_US".len());
            loc_name.push_str("loc");
            tbl.push((format!("{}: {}", loc_name, col_ty), not_null));
        }
        if let Some(x) = fk_table {
            let rev_col = format!("{}_{}", table_name, col_name);

            rev_rels
                .entry(x.clone())
                .or_default()
                .push(format!("{}: [{}!]", rev_col, table_name));
        }
        tbl.push((format!("{}: {}", col_name, col_ty), not_null));
    }
    for (table_name, cols) in &table_rels {
        write!(out, "type {} {{\n", table_name)?;
        for (col_ty, not_null) in cols {
            write!(out, "\t{}", col_ty)?;
            if *not_null {
                write!(out, "!")?;
            }
            write!(out, "\n")?;
        }
        write!(out, "}}\n\n")?;
    }
    write!(
        out,
        "type Query {{\n\t{}\n}}",
        table_rels
            .into_iter()
            .map(|(k, v)| format!(
                "{}({}): [{}]",
                k,
                v.into_iter().map(|x| x.0).collect::<Vec<_>>().join(", "),
                k
            ))
            .collect::<Vec<_>>()
            .join("\n\t")
    )?;
    Ok(out)
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let opts: Options = argh::from_env();
    let start = Instant::now();

    let mut conn = Connection::open(opts.src)?;

    let out = try_export_db(&mut conn).wrap_err("Failed to export database to sqlite")?;

    std::fs::write(opts.dest, out)?;

    let duration = start.elapsed();
    println!(
        "Finished in {}.{}s",
        duration.as_secs(),
        duration.subsec_millis()
    );

    Ok(())
}
