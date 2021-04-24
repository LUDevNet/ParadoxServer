use std::{
    convert::{Infallible, TryFrom},
    fs::File,
    path::PathBuf,
};

use assembly_data::fdb::{
    common::{Latin1Str, Latin1String, ValueType},
    file::FDBHeader,
    mem, query,
    ro::{ArcHandle, Handle},
};
use color_eyre::eyre::WrapErr;
use linked_hash_map::LinkedHashMap;
use mapr::Mmap;
use serde::Deserialize;
use structopt::StructOpt;
use warp::{
    reply::{Json, WithStatus},
    Filter, Rejection, Reply,
};

fn default_port() -> u16 {
    3030
}

#[derive(Deserialize)]
struct CorsOptions {
    all: bool,
    domains: Vec<String>,
}

impl Default for CorsOptions {
    fn default() -> Self {
        Self {
            all: true,
            domains: vec![],
        }
    }
}

#[derive(Deserialize)]
struct GeneralOptions {
    /// The port for the server
    #[serde(default = "default_port")]
    port: u16,
    /// Bind to `0.0.0.0` instead of `127.0.0.1`
    public: bool,
    /// The allowed cross-origin domains
    #[serde(default)]
    cors: CorsOptions,
    /// The base of the path
    base: String,
}

#[derive(Deserialize)]
struct TlsOptions {
    /// Whether TLS is enabled
    enabled: bool,
    /// The private key file
    key: PathBuf,
    /// The certificate file
    cert: PathBuf,
}

#[derive(Deserialize)]
struct DataOptions {
    /// The CDClient database FDB file
    cdclient: PathBuf,
    /// The lu-explorer static files
    explorer_spa: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    general: GeneralOptions,
    tls: Option<TlsOptions>,
    data: DataOptions,
}

#[derive(StructOpt)]
/// Starts the server that serves a JSON API to the client files
struct Options {
    #[structopt(long, default_value = "paradox.toml")]
    cfg: PathBuf,
}

fn table_index(db_table: Handle<'_, FDBHeader>, lname: &Latin1Str, key: String) -> Json {
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
}

fn tables_api<B: AsRef<[u8]>>(db: ArcHandle<B, FDBHeader>) -> WithStatus<Json> {
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
}

fn table_def_api<B: AsRef<[u8]>>(
    db_table: ArcHandle<B, FDBHeader>,
    name: String,
) -> WithStatus<Json> {
    let lname = Latin1String::encode(&name);
    let db_table = db_table.as_bytes_handle();
    let table = db_table.into_table_by_name(lname.as_ref()).unwrap();

    if let Some(table) = table.transpose() {
        let table_def = table.into_definition().unwrap();
        return wrap_200(warp::reply::json(&table_def));
    }
    wrap_404(warp::reply::json(&()))
}

fn table_key_api<B: AsRef<[u8]>>(
    db_table: ArcHandle<B, FDBHeader>,
    name: String,
    key: String,
) -> WithStatus<Json> {
    let lname = Latin1String::encode(&name);
    let db_table = db_table.as_bytes_handle();

    wrap_200(table_index(db_table, lname.as_ref(), key))
}

fn wrap_404<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND)
}

fn wrap_200<A: Reply>(reply: A) -> WithStatus<A> {
    warp::reply::with_status(reply, warp::http::StatusCode::OK)
}

fn make_api_catch_all() -> impl Filter<Extract = (WithStatus<Json>,), Error = Infallible> + Clone
{
    warp::any().map(|| warp::reply::json(&404)).map(wrap_404)
}

fn make_v0() -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::path("v0")
}

fn make_tables<B>(
    hnd: ArcHandle<B, FDBHeader>,
) -> impl Filter<Extract = (ArcHandle<B, FDBHeader>,), Error = Infallible> + Clone + Send
where
    B: AsRef<[u8]> + Send + Sync,
{
    warp::any().map(move || hnd.clone())
}

fn make_api_tables<B, H>(
    hnd_state: H,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Send
where
    B: AsRef<[u8]> + Send + Sync,
    H: Filter<Extract = (ArcHandle<B, FDBHeader>,), Error = Infallible> + Clone + Send,
{
    let tables_base = hnd_state;

    // The `/tables` endpoint
    let tables = tables_base.clone().and(warp::path::end()).map(tables_api);
    let table = tables_base.and(warp::path::param());

    // The `/tables/:name/def` endpoint
    let table_def = table.clone().and(warp::path("def")).map(table_def_api);
    // The `/tables/:name/:key` endpoint
    let table_get = table.and(warp::path::param()).map(table_key_api);

    tables.or(table_def).unify().or(table_get).unify()
}

fn make_api<B: AsRef<[u8]> + Send + Sync>(
    hnd: ArcHandle<B, FDBHeader>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Infallible> + Clone {
    let api_base = make_v0();
    let hnd_state = make_tables(hnd);
    let tables = warp::path("tables").and(make_api_tables(hnd_state));
    let catch_all = make_api_catch_all();

    api_base.and(tables).or(catch_all).unify()
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    pretty_env_logger::init();

    color_eyre::install()?;
    let opts = Options::from_args();

    let cfg_path = opts.cfg;
    let cfg_file = std::fs::read_to_string(&cfg_path)
        .wrap_err_with(|| format!("Failed to open config file '{}'", cfg_path.display()))?;
    let cfg: Config = toml::from_str(&cfg_file)?;

    // Load the database file
    let file = File::open(&cfg.data.cdclient).wrap_err_with(|| {
        format!(
            "Failed to open input file '{}'",
            cfg.data.cdclient.display()
        )
    })?;
    let hnd = ArcHandle::new_arc(unsafe { Mmap::map(&file)? });
    let hnd = hnd.into_tables()?;

    let base = warp::path(cfg.general.base);
    let api = warp::path("api").and(make_api(hnd));

    let spa_path = cfg.data.explorer_spa;
    let spa_index = spa_path.join("index.html");
    let spa = warp::fs::dir(spa_path).or(warp::fs::file(spa_index));

    let routes = warp::get().and(base).and(api.or(spa));

    let ip = if cfg.general.public {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };

    let mut cors = warp::cors();
    let cors_cfg = &cfg.general.cors;
    if cors_cfg.all {
        cors = cors.allow_any_origin();
    } else {
        for key in &cors_cfg.domains {
            cors = cors.allow_origin(key.as_ref());
        }
    }
    cors = cors.allow_methods(vec!["GET"]);
    let to_serve = routes.with(cors);
    let server = warp::serve(to_serve);

    if let Some(tls_cfg) = cfg.tls {
        if tls_cfg.enabled {
            server
                .tls()
                .key_path(tls_cfg.key)
                .cert_path(tls_cfg.cert)
                .run((ip, cfg.general.port))
                .await;
            return Ok(());
        }
    }
    server.run((ip, cfg.general.port)).await;

    Ok(())
}
