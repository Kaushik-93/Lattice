use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;
use calamine::Reader as _;
use duckdb::types::Value;
use duckdb::{Config, Connection, params_from_iter};
use lattice_connectors::{FileFormat, ident, literal, read_file, table_name};
use lattice_planner::ParamValue;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{EXTENSIONS, EngineConfig, EngineError, path_str};

/// Loads and copies can run far longer than interactive queries.
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// Live tables are profiled from their first rows, so profiling never scans a warehouse.
pub const LIVE_PROFILE_ROWS: u64 = 100_000;

/// Statements that attach one live source when the project database opens.
#[derive(Debug, Clone)]
pub struct Registration {
    pub alias: String,
    pub statements: Vec<String>,
}

#[derive(Debug)]
pub struct QueryOutput {
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    pub rows: usize,
    /// Waiting for a free connection.
    pub queued: Duration,
    /// Running in DuckDB, including materialising the result as Arrow.
    pub executed: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntrospectedTable {
    pub schema: String,
    pub name: String,
    pub estimated_rows: Option<i64>,
    pub columns: Vec<IntrospectedColumn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntrospectedColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfile {
    pub name: String,
    pub data_type: String,
    pub min: Option<String>,
    pub max: Option<String>,
    pub approx_distinct: Option<i64>,
    pub null_fraction: Option<f64>,
    pub avg: Option<String>,
    pub samples: Vec<String>,
}

/// One open project database. Cheap to share; every method is safe to call concurrently.
pub struct ProjectDb {
    id: Uuid,
    dir: PathBuf,
    /// Never queried; only cloned when a pooled connection is lost.
    base: Mutex<Connection>,
    pool: Mutex<Vec<Connection>>,
    permits: Semaphore,
    timeout: Duration,
    attach_errors: Vec<(String, String)>,
}

impl ProjectDb {
    pub(crate) fn open(
        id: Uuid,
        config: &EngineConfig,
        registrations: &[Registration],
    ) -> Result<Self, EngineError> {
        let dir = config.project_dir(id);
        std::fs::create_dir_all(dir.join("uploads"))?;
        // Canonical, so the allow-list matches the paths we hand DuckDB (/var vs /private/var on macOS).
        let dir = dir.canonicalize()?;

        let mut db_config = Config::default()
            .with("extension_directory", path_str(&config.extension_dir)?)?
            .with("autoinstall_known_extensions", "false")?
            .with("autoload_known_extensions", "false")?
            .max_memory(&config.memory_limit)?;
        if let Some(threads) = config.threads {
            db_config = db_config.threads(i64::from(threads))?;
        }
        let conn = Connection::open_with_flags(dir.join("project.duckdb"), db_config)?;
        for ext in EXTENSIONS {
            conn.execute_batch(&format!("LOAD {ext}"))?;
        }

        let mut attach_errors = Vec::new();
        for reg in registrations {
            if let Some(err) = reg
                .statements
                .iter()
                .find_map(|s| conn.execute_batch(s).err())
            {
                // One unreachable source must not take the whole project down.
                tracing::warn!(project = %id, source = %reg.alias, error = %err, "could not attach source");
                attach_errors.push((reg.alias.clone(), err.to_string()));
            }
        }

        // From here on SQL can only touch this project's directory, and can't undo that.
        let allowed = format!("{}/", path_str(&dir)?);
        conn.execute_batch(&format!(
            "SET allowed_directories = [{}]; SET enable_external_access = false; SET lock_configuration = true;",
            literal(&allowed)
        ))?;

        let pool = (0..config.pool_size)
            .map(|_| conn.try_clone())
            .collect::<Result<Vec<_>, _>>()?;
        tracing::info!(project = %id, sources = registrations.len(), failed = attach_errors.len(), "opened project database");
        Ok(Self {
            id,
            dir,
            base: Mutex::new(conn),
            pool: Mutex::new(pool),
            permits: Semaphore::new(config.pool_size),
            timeout: config.query_timeout,
            attach_errors,
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Canonical project directory: the only place SQL may read or write files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn uploads_dir(&self) -> PathBuf {
        self.dir.join("uploads")
    }

    /// Sources that failed to attach when the database opened, as (alias, error).
    pub fn attach_errors(&self) -> &[(String, String)] {
        &self.attach_errors
    }

    /// Runs one query and returns its result as Arrow.
    pub async fn query(
        &self,
        sql: String,
        params: Vec<ParamValue>,
        cancel: &CancellationToken,
    ) -> Result<QueryOutput, EngineError> {
        let ((schema, batches), queued, executed) = self
            .with_conn(cancel, self.timeout, move |conn| {
                let mut stmt = conn.prepare_cached(&sql)?;
                let values: Vec<Value> = params.iter().map(to_value).collect();
                let arrow = stmt.query_arrow(params_from_iter(values.iter()))?;
                let schema = arrow.get_schema();
                Ok((schema, arrow.collect::<Vec<_>>()))
            })
            .await?;
        let rows = batches.iter().map(RecordBatch::num_rows).sum();
        Ok(QueryOutput {
            schema,
            batches,
            rows,
            queued,
            executed,
        })
    }

    /// Runs statements that return nothing (DDL, loads).
    pub async fn execute(&self, sql: String) -> Result<(), EngineError> {
        self.with_conn(&CancellationToken::new(), LOAD_TIMEOUT, move |conn| {
            conn.execute_batch(&sql)?;
            Ok(())
        })
        .await
        .map(|(v, _, _)| v)
    }

    /// Loads an uploaded file (already inside `uploads_dir`) into native tables in `schema`.
    /// Excel workbooks become one table per sheet. Returns (table, rows) pairs.
    pub async fn load_file(
        &self,
        path: PathBuf,
        format: FileFormat,
        schema: String,
    ) -> Result<Vec<(String, i64)>, EngineError> {
        let path = path.canonicalize()?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sources: Vec<(String, Option<String>)> = match format {
            FileFormat::Xlsx => calamine::open_workbook_auto(&path)
                .map_err(|e| EngineError::Spreadsheet(e.to_string()))?
                .sheet_names()
                .into_iter()
                .map(|sheet| (table_name(&sheet), Some(sheet)))
                .collect(),
            _ => vec![(table_name(&file_name), None)],
        };
        let path = path_str(&path)?.to_owned();
        self.with_conn(&CancellationToken::new(), LOAD_TIMEOUT, move |conn| {
            conn.execute_batch(&format!("CREATE SCHEMA IF NOT EXISTS {}", ident(&schema)))?;
            let mut used = HashSet::new();
            let mut loaded = Vec::new();
            for (base, sheet) in sources {
                let table = unique(&mut used, &base);
                let target = format!("{}.{}", ident(&schema), ident(&table));
                conn.execute_batch(&format!(
                    "CREATE OR REPLACE TABLE {target} AS SELECT * FROM {}",
                    read_file(format, &path, sheet.as_deref())
                ))?;
                let rows: i64 =
                    conn.query_row(&format!("SELECT count(*) FROM {target}"), [], |r| r.get(0))?;
                loaded.push((table, rows));
            }
            Ok(loaded)
        })
        .await
        .map(|(v, _, _)| v)
    }

    /// `CREATE OR REPLACE TABLE schema.table AS <select>`; returns the row count. Used for
    /// extracts of live tables and for generated demo data.
    pub async fn create_table_as(
        &self,
        schema: String,
        table: String,
        select: String,
    ) -> Result<i64, EngineError> {
        self.with_conn(&CancellationToken::new(), LOAD_TIMEOUT, move |conn| {
            let target = format!("{}.{}", ident(&schema), ident(&table));
            conn.execute_batch(&format!(
                "CREATE SCHEMA IF NOT EXISTS {}; CREATE OR REPLACE TABLE {target} AS {select}",
                ident(&schema)
            ))?;
            Ok(conn.query_row(&format!("SELECT count(*) FROM {target}"), [], |r| r.get(0))?)
        })
        .await
        .map(|(v, _, _)| v)
    }

    /// Writes a table to a zstd Parquet file inside the project directory; returns its size.
    pub async fn export_parquet(
        &self,
        table_ref: String,
        out: PathBuf,
    ) -> Result<u64, EngineError> {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out_str = path_str(&out)?.to_owned();
        self.with_conn(&CancellationToken::new(), LOAD_TIMEOUT, move |conn| {
            conn.execute_batch(&format!(
                "COPY (SELECT * FROM {table_ref}) TO {} (FORMAT parquet, COMPRESSION zstd)",
                literal(&out_str)
            ))?;
            Ok(())
        })
        .await?;
        Ok(std::fs::metadata(&out)?.len())
    }

    /// Tables and columns of one attached database (or of `project` for extracts), optionally
    /// in one schema.
    pub async fn introspect(
        &self,
        database: String,
        schema: Option<String>,
    ) -> Result<Vec<IntrospectedTable>, EngineError> {
        self.with_conn(&CancellationToken::new(), self.timeout, move |conn| {
            introspect_conn(conn, &database, schema.as_deref())
        })
        .await
        .map(|(v, _, _)| v)
    }

    /// Per-column statistics (DuckDB `SUMMARIZE`) plus a few sample values. With `row_cap`, only
    /// that many leading rows are profiled. Returns the profiles and the rows profiled.
    pub async fn profile(
        &self,
        table_ref: String,
        row_cap: Option<u64>,
    ) -> Result<(Vec<ColumnProfile>, i64), EngineError> {
        self.with_conn(&CancellationToken::new(), LOAD_TIMEOUT, move |conn| {
            let source = match row_cap {
                Some(n) => format!("(SELECT * FROM {table_ref} LIMIT {n})"),
                None => table_ref,
            };
            let mut stmt = conn.prepare(&format!(
                "SELECT column_name, column_type, min, max, approx_unique, avg,
                        CAST(null_percentage AS DOUBLE), count
                 FROM (SUMMARIZE SELECT * FROM {source})"
            ))?;
            let mut rows = stmt.query([])?;
            let mut profiles = Vec::new();
            let mut row_count = 0i64;
            while let Some(row) = rows.next()? {
                row_count = row.get::<_, Option<i64>>(7)?.unwrap_or(0);
                profiles.push(ColumnProfile {
                    name: row.get(0)?,
                    data_type: row.get(1)?,
                    min: row.get(2)?,
                    max: row.get(3)?,
                    approx_distinct: row.get(4)?,
                    avg: row.get(5)?,
                    null_fraction: row.get::<_, Option<f64>>(6)?.map(|p| p / 100.0),
                    samples: Vec::new(),
                });
            }
            if !profiles.is_empty() {
                let columns = profiles
                    .iter()
                    .map(|p| format!("CAST({} AS VARCHAR)", ident(&p.name)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut stmt = conn.prepare(&format!(
                    "SELECT {columns} FROM (SELECT * FROM {source} LIMIT 1000) USING SAMPLE 5 ROWS (reservoir, 42)"
                ))?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    for (i, profile) in profiles.iter_mut().enumerate() {
                        if let Some(v) = row.get::<_, Option<String>>(i)?
                            && !profile.samples.contains(&v)
                        {
                            profile.samples.push(v.chars().take(80).collect());
                        }
                    }
                }
            }
            Ok((profiles, row_count))
        })
        .await
        .map(|(v, _, _)| v)
    }

    /// Checks out a connection, runs `f` on a blocking thread, and returns
    /// (result, time queued, time executing). Cancellation and the timeout interrupt DuckDB.
    async fn with_conn<T, F>(
        &self,
        cancel: &CancellationToken,
        timeout: Duration,
        f: F,
    ) -> Result<(T, Duration, Duration), EngineError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, EngineError> + Send + 'static,
    {
        let queued_at = Instant::now();
        let _permit = tokio::select! {
            permit = self.permits.acquire() => permit.map_err(|_| EngineError::Busy)?,
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
        };
        let queued = queued_at.elapsed();
        let conn = self.checkout()?;
        let interrupt = conn.interrupt_handle();
        let mut task = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let result = f(&conn);
            (conn, result, started.elapsed())
        });
        let stopped = tokio::select! {
            joined = &mut task => {
                let (conn, result, executed) = joined?;
                self.checkin(conn);
                return result.map(|v| (v, queued, executed));
            }
            () = cancel.cancelled() => EngineError::Cancelled,
            () = tokio::time::sleep(timeout) => EngineError::Timeout(timeout),
        };
        interrupt.interrupt();
        // DuckDB stops within milliseconds of an interrupt; take the connection back.
        if let Ok((conn, _, _)) = task.await {
            self.checkin(conn);
        }
        Err(stopped)
    }

    fn checkout(&self) -> Result<Connection, EngineError> {
        let pooled = self.pool.lock().unwrap_or_else(|e| e.into_inner()).pop();
        match pooled {
            Some(conn) => Ok(conn),
            // Only after a blocking task panicked and took its connection with it.
            None => Ok(self
                .base
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_clone()?),
        }
    }

    fn checkin(&self, conn: Connection) {
        self.pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(conn);
    }
}

pub(crate) fn to_value(p: &ParamValue) -> Value {
    match p {
        ParamValue::Bool(b) => Value::Boolean(*b),
        ParamValue::BigInt(i) => Value::BigInt(*i),
        ParamValue::Double(d) => Value::Double(*d),
        ParamValue::Text(s) => Value::Text(s.clone()),
    }
}

fn unique(used: &mut HashSet<String>, base: &str) -> String {
    let mut name = base.to_owned();
    let mut n = 2;
    while !used.insert(name.clone()) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

pub(crate) fn introspect_conn(
    conn: &Connection,
    database: &str,
    schema: Option<&str>,
) -> Result<Vec<IntrospectedTable>, EngineError> {
    let mut estimates = std::collections::HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT schema_name, table_name, estimated_size FROM duckdb_tables()
         WHERE database_name = ? AND (?::VARCHAR IS NULL OR schema_name = ?)",
    )?;
    let mut rows = stmt.query(duckdb::params![database, schema, schema])?;
    while let Some(row) = rows.next()? {
        let key: (String, String) = (row.get(0)?, row.get(1)?);
        estimates.insert(key, row.get::<_, Option<i64>>(2)?);
    }

    let mut stmt = conn.prepare(
        "SELECT schema_name, table_name, column_name, data_type, is_nullable
         FROM duckdb_columns()
         WHERE database_name = ? AND (?::VARCHAR IS NULL OR schema_name = ?)
           AND NOT internal AND schema_name NOT IN ('information_schema', 'pg_catalog')
         ORDER BY schema_name, table_name, column_index",
    )?;
    let mut rows = stmt.query(duckdb::params![database, schema, schema])?;
    let mut tables: Vec<IntrospectedTable> = Vec::new();
    while let Some(row) = rows.next()? {
        let (schema_name, table): (String, String) = (row.get(0)?, row.get(1)?);
        let column = IntrospectedColumn {
            name: row.get(2)?,
            data_type: row.get(3)?,
            nullable: row.get::<_, Option<bool>>(4)?.unwrap_or(true),
        };
        match tables.last_mut() {
            Some(t) if t.schema == schema_name && t.name == table => t.columns.push(column),
            _ => tables.push(IntrospectedTable {
                estimated_rows: estimates
                    .get(&(schema_name.clone(), table.clone()))
                    .copied()
                    .flatten(),
                schema: schema_name,
                name: table,
                columns: vec![column],
            }),
        }
    }
    Ok(tables)
}
