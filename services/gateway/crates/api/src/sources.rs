//! Source lifecycle: uploads and database connections become catalog entries backed by the
//! project's DuckDB database (docs/adr/0004).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lattice_catalog::{
    self as catalog, ColumnSpec, ColumnStats, NewSource, SourceDetail, SourceRecord, SourceStatus,
    TableSpec,
};
use lattice_connectors::{
    DatabaseConfig, SourceKind, TableMode, UploadKind, attach_database, attach_sqlite, ident,
};
use lattice_engine::{
    ColumnProfile, IntrospectedTable, LIVE_PROFILE_ROWS, ProjectDb, Registration,
};
use lattice_security::Principal;
use lattice_security::audit::{self, Actor, AuditEvent};
use object_store::path::Path as ObjectPath;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;

use crate::dto::CreateDatabaseSourceRequest;
use crate::error::ApiError;
use crate::state::AppState;

/// Tables profiled automatically when a source is added; the rest are profiled on demand.
const AUTO_PROFILE_TABLES: usize = 50;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// DuckDB schema holding a source's extracts inside `project.duckdb`.
pub fn extract_schema(source_id: Uuid) -> String {
    format!("s_{}", source_id.simple())
}

/// DuckDB alias of an attached live source.
pub fn live_alias(source_id: Uuid) -> String {
    format!("src_{}", source_id.simple())
}

/// Fully quoted DuckDB reference to a catalog table.
pub fn table_reference(kind: SourceKind, source_id: Uuid, schema: &str, table: &str) -> String {
    let database = match kind.mode() {
        TableMode::Extract => "project".to_owned(),
        TableMode::Live => live_alias(source_id),
    };
    format!("{}.{}.{}", ident(&database), ident(schema), ident(table))
}

#[derive(Serialize, Deserialize)]
struct StoredSecret {
    password: String,
}

/// The project's database, opened (with its live sources attached) on first use.
pub async fn project_db(state: &AppState, project_id: Uuid) -> Result<Arc<ProjectDb>, ApiError> {
    if let Some(db) = state.engine.get(project_id).await {
        return Ok(db);
    }
    let sources = catalog::live_sources(&state.pool, project_id).await?;
    let mut registrations = Vec::with_capacity(sources.len());
    for source in &sources {
        match registration(state, project_id, source) {
            Ok(reg) => registrations.push(reg),
            Err(err) => {
                tracing::warn!(source = %source.id, error = %err, "skipping source that can't be attached")
            }
        }
    }
    let db = state.engine.open(project_id, registrations).await?;
    for (alias, error) in db.attach_errors() {
        if let Some(source) = sources.iter().find(|s| live_alias(s.id) == *alias) {
            catalog::set_status(
                &state.pool,
                source.id,
                SourceStatus::Error,
                Some(&short(error)),
            )
            .await?;
        }
    }
    Ok(db)
}

fn registration(
    state: &AppState,
    project_id: Uuid,
    source: &SourceRecord,
) -> Result<Registration, ApiError> {
    let alias = live_alias(source.id);
    let statements = match source.kind {
        SourceKind::Sqlite => {
            let relative = source
                .config
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or_default();
            let path = state.engine.config().project_dir(project_id).join(relative);
            let path = path.canonicalize().unwrap_or(path);
            attach_sqlite(&alias, &path.to_string_lossy())
        }
        SourceKind::Postgres | SourceKind::Mysql => {
            let config: DatabaseConfig = serde_json::from_value(source.config.clone())?;
            attach_database(source.kind, &alias, &config, &open_password(state, source)?)
        }
        SourceKind::File | SourceKind::Demo => return Err(ApiError::internal("not a live source")),
    };
    Ok(Registration { alias, statements })
}

fn open_password(state: &AppState, source: &SourceRecord) -> Result<String, ApiError> {
    let secrets = state.secrets.as_ref().ok_or_else(missing_key)?;
    let sealed = source
        .secret
        .as_deref()
        .ok_or_else(|| ApiError::internal("source has no stored credentials"))?;
    let plain = secrets.open(sealed, source.id.as_bytes())?;
    Ok(serde_json::from_slice::<StoredSecret>(&plain)?.password)
}

fn missing_key() -> ApiError {
    ApiError::Unprocessable(
        "Database sources need LATTICE_SECRET_KEY configured on the gateway.".into(),
    )
}

/// Turns an upload (already streamed to `incoming`) into a source.
pub async fn create_file_source(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    incoming: PathBuf,
    file_name: &str,
    size_bytes: u64,
) -> Result<SourceDetail, ApiError> {
    let kind = UploadKind::from_file_name(file_name).ok_or_else(unsupported_file)?;
    let (source_kind, mut config) = match kind {
        UploadKind::File(format) => (
            SourceKind::File,
            json!({ "format": format, "file_name": file_name, "size_bytes": size_bytes }),
        ),
        UploadKind::Sqlite => (
            SourceKind::Sqlite,
            json!({ "file_name": file_name, "size_bytes": size_bytes }),
        ),
    };
    let source_id = catalog::insert_source(
        &state.pool,
        NewSource {
            project_id,
            kind: source_kind,
            name: &file_name.chars().take(120).collect::<String>(),
            config: config.clone(),
            created_by: me.user_id,
        },
    )
    .await?;

    let result = async {
        let db = project_db(state, project_id).await?;
        let dir = db.uploads_dir().join(source_id.simple().to_string());
        tokio::fs::create_dir_all(&dir).await?;
        let stored = dir.join(safe_file_name(file_name));
        tokio::fs::rename(&incoming, &stored).await?;
        let relative = stored
            .strip_prefix(db.dir())
            .unwrap_or(&stored)
            .to_string_lossy()
            .into_owned();
        config["path"] = json!(relative);
        catalog::set_config(&state.pool, source_id, &config).await?;

        match kind {
            UploadKind::File(format) => {
                let schema = extract_schema(source_id);
                let loaded = db.load_file(stored.clone(), format, schema.clone()).await?;
                let tables = db.introspect("project".into(), Some(schema)).await?;
                let tables = with_counts(tables, |t| {
                    loaded.iter().find(|(n, _)| *n == t.name).map(|(_, r)| *r)
                });
                sync(state, &db, source_id, SourceKind::File, &tables).await?;
            }
            UploadKind::Sqlite => {
                // Attaching needs a fresh project database (configuration is locked once open).
                drop(db);
                state.engine.invalidate(project_id).await;
                let db = project_db(state, project_id).await?;
                ensure_attached(&db, source_id)?;
                let tables = db.introspect(live_alias(source_id), None).await?;
                sync(state, &db, source_id, SourceKind::Sqlite, &tables).await?;
            }
        }
        backup(state, project_id, source_id, &stored);
        Ok::<_, ApiError>(())
    }
    .await;

    finish(state, me, project_id, source_id, "source.upload", result).await
}

/// Adds a PostgreSQL or MySQL database after checking it can be reached with the given credentials.
pub async fn create_database_source(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    req: CreateDatabaseSourceRequest,
) -> Result<SourceDetail, ApiError> {
    if !matches!(req.kind, SourceKind::Postgres | SourceKind::Mysql) {
        return Err(ApiError::BadRequest(
            "kind must be postgres or mysql".into(),
        ));
    }
    let name = crate::auth::password::required_text(&req.name, "Name", 120)?.to_owned();
    req.config.validate()?;
    req.config.guard_metadata(&state.metadata_db)?;
    let secrets = state.secrets.as_ref().ok_or_else(missing_key)?;

    // Probe on a throwaway database first: bad credentials fail here and nothing is saved.
    let alias = format!("probe_{}", Uuid::new_v4().simple());
    let probe = Registration {
        statements: attach_database(req.kind, &alias, &req.config, &req.password),
        alias,
    };
    let tables = state
        .engine
        .probe(probe, req.config.schema.clone(), PROBE_TIMEOUT)
        .await
        .map_err(|e| {
            ApiError::Unprocessable(format!("Couldn't connect: {}", short(&e.to_string())))
        })?;

    let source_id = catalog::insert_source(
        &state.pool,
        NewSource {
            project_id,
            kind: req.kind,
            name: &name,
            config: serde_json::to_value(&req.config)?,
            created_by: me.user_id,
        },
    )
    .await?;
    let sealed = secrets.seal(
        &serde_json::to_vec(&StoredSecret {
            password: req.password,
        })?,
        source_id.as_bytes(),
    );
    catalog::set_secret(&state.pool, source_id, &sealed).await?;

    let result = async {
        state.engine.invalidate(project_id).await;
        let db = project_db(state, project_id).await?;
        ensure_attached(&db, source_id)?;
        // Live warehouses aren't profiled up front (that would scan them); profile on demand.
        let specs = to_specs(&tables);
        catalog::sync_tables(&state.pool, source_id, TableMode::Live, &specs).await?;
        Ok::<_, ApiError>(())
    }
    .await;

    finish(state, me, project_id, source_id, "source.connect", result).await
}

/// Re-reads a source's schema (and reattaches live sources); bumps its data version.
pub async fn refresh(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    source_id: Uuid,
) -> Result<SourceDetail, ApiError> {
    let source = catalog::source_record(&state.pool, project_id, source_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    catalog::set_status(&state.pool, source_id, SourceStatus::Syncing, None).await?;
    let result = async {
        let (database, schema) = match source.kind.mode() {
            TableMode::Extract => ("project".to_owned(), Some(extract_schema(source_id))),
            TableMode::Live => {
                state.engine.invalidate(project_id).await;
                let schema = source
                    .config
                    .get("schema")
                    .and_then(|s| s.as_str())
                    .map(str::to_owned);
                (live_alias(source_id), schema)
            }
        };
        let db = project_db(state, project_id).await?;
        if source.kind.mode() == TableMode::Live {
            ensure_attached(&db, source_id)?;
        }
        let tables = db.introspect(database, schema).await?;
        let specs = to_specs(&tables);
        catalog::sync_tables(&state.pool, source_id, source.kind.mode(), &specs).await?;
        Ok::<_, ApiError>(())
    }
    .await;
    finish(state, me, project_id, source_id, "source.refresh", result).await
}

pub async fn delete(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    source_id: Uuid,
) -> Result<(), ApiError> {
    let source = catalog::source_record(&state.pool, project_id, source_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    catalog::delete_source(&state.pool, project_id, source_id).await?;
    match source.kind.mode() {
        TableMode::Extract => {
            let db = project_db(state, project_id).await?;
            db.execute(format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                ident(&extract_schema(source_id))
            ))
            .await?;
        }
        TableMode::Live => state.engine.invalidate(project_id).await,
    }
    let dir = state
        .engine
        .config()
        .project_dir(project_id)
        .join("uploads")
        .join(source_id.simple().to_string());
    if let Err(err) = tokio::fs::remove_dir_all(&dir).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %err, "could not remove source files");
    }
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action: "source.delete",
            target: Some(format!("source:{source_id}")),
            detail: json!({ "name": source.name, "project_id": project_id }),
        },
    )
    .await?;
    Ok(())
}

/// Profiles one table on demand; live tables are profiled from their first rows only.
pub async fn profile_table(
    state: &AppState,
    project_id: Uuid,
    table_id: Uuid,
) -> Result<SourceDetail, ApiError> {
    let table = catalog::resolve_table(&state.pool, project_id, table_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let db = project_db(state, project_id).await?;
    let reference = table_reference(
        table.kind,
        table.source_id,
        &table.schema_name,
        &table.table_name,
    );
    let cap = (table.kind.mode() == TableMode::Live).then_some(LIVE_PROFILE_ROWS);
    let (profiles, rows) = db.profile(reference, cap).await?;
    // A capped profile counts only the rows it read; keep the table's own estimate instead.
    store_profile(state, table_id, cap.is_none().then_some(rows), &profiles).await?;
    catalog::source_detail(&state.pool, project_id, table.source_id)
        .await?
        .ok_or(ApiError::NotFound)
}

pub(crate) async fn sync(
    state: &AppState,
    db: &ProjectDb,
    source_id: Uuid,
    kind: SourceKind,
    tables: &[IntrospectedTable],
) -> Result<(), ApiError> {
    let ids = catalog::sync_tables(&state.pool, source_id, kind.mode(), &to_specs(tables)).await?;
    for (table, id) in tables.iter().zip(ids).take(AUTO_PROFILE_TABLES) {
        let reference = table_reference(kind, source_id, &table.schema, &table.name);
        let cap = (kind.mode() == TableMode::Live).then_some(LIVE_PROFILE_ROWS);
        let (profiles, rows) = db.profile(reference, cap).await?;
        let rows = if cap.is_none() {
            Some(rows)
        } else {
            table.estimated_rows
        };
        store_profile(state, id, rows, &profiles).await?;
    }
    Ok(())
}

async fn store_profile(
    state: &AppState,
    table_id: Uuid,
    rows: Option<i64>,
    profiles: &[ColumnProfile],
) -> Result<(), ApiError> {
    let stats: Vec<(String, ColumnStats)> = profiles
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                ColumnStats {
                    min: p.min.clone(),
                    max: p.max.clone(),
                    approx_distinct: p.approx_distinct,
                    null_fraction: p.null_fraction,
                    avg: p.avg.clone(),
                    samples: p.samples.clone(),
                },
            )
        })
        .collect();
    catalog::set_profile(&state.pool, table_id, rows, &stats).await?;
    Ok(())
}

fn to_specs(tables: &[IntrospectedTable]) -> Vec<TableSpec> {
    tables
        .iter()
        .map(|t| TableSpec {
            schema: t.schema.clone(),
            name: t.name.clone(),
            // Live scanners report unknown sizes as 0; unknown is not empty.
            row_count: t.estimated_rows.filter(|n| *n > 0),
            columns: t
                .columns
                .iter()
                .map(|c| ColumnSpec {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                    nullable: c.nullable,
                })
                .collect(),
        })
        .collect()
}

fn with_counts(
    mut tables: Vec<IntrospectedTable>,
    count: impl Fn(&IntrospectedTable) -> Option<i64>,
) -> Vec<IntrospectedTable> {
    for t in &mut tables {
        if let Some(rows) = count(t) {
            t.estimated_rows = Some(rows);
        }
    }
    tables
}

fn ensure_attached(db: &ProjectDb, source_id: Uuid) -> Result<(), ApiError> {
    let alias = live_alias(source_id);
    match db.attach_errors().iter().find(|(a, _)| *a == alias) {
        Some((_, err)) => Err(ApiError::Unprocessable(format!(
            "Couldn't attach the source: {}",
            short(err)
        ))),
        None => Ok(()),
    }
}

/// Records the outcome on the source and in the audit log, then returns the source.
async fn finish(
    state: &AppState,
    me: &Principal,
    project_id: Uuid,
    source_id: Uuid,
    action: &str,
    result: Result<(), ApiError>,
) -> Result<SourceDetail, ApiError> {
    match &result {
        Ok(()) => catalog::set_status(&state.pool, source_id, SourceStatus::Ready, None).await?,
        Err(err) => {
            catalog::set_status(
                &state.pool,
                source_id,
                SourceStatus::Error,
                Some(&short(&err.to_string())),
            )
            .await?;
        }
    }
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(me.workspace_id),
            actor: Actor::User(me.user_id),
            action,
            target: Some(format!("source:{source_id}")),
            detail: json!({ "project_id": project_id, "ok": result.is_ok() }),
        },
    )
    .await?;
    result?;
    catalog::source_detail(&state.pool, project_id, source_id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Copies the original upload to object storage in the background (durability; best effort).
fn backup(state: &AppState, project_id: Uuid, source_id: Uuid, file: &Path) {
    let Some(store) = state.store.clone() else {
        return;
    };
    let file = file.to_path_buf();
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    tokio::spawn(async move {
        let key = ObjectPath::from(format!("projects/{project_id}/sources/{source_id}/{name}"));
        let upload = async {
            let mut reader = tokio::fs::File::open(&file).await?;
            let mut writer = object_store::buffered::BufWriter::new(store, key.clone());
            tokio::io::copy(&mut reader, &mut writer).await?;
            writer.shutdown().await?;
            Ok::<_, std::io::Error>(())
        };
        match upload.await {
            Ok(()) => tracing::info!(%key, "stored original upload"),
            Err(err) => {
                tracing::warn!(%key, error = %err, "could not copy upload to object storage")
            }
        }
    });
}

/// The last path component of an uploaded name, without separators or control characters, so
/// it can never escape the upload directory.
pub fn safe_file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    if cleaned.is_empty() {
        "upload".to_owned()
    } else {
        cleaned
    }
}

fn unsupported_file() -> ApiError {
    ApiError::BadRequest(
        "Lattice reads CSV, TSV, JSON, NDJSON, Parquet, Excel (.xlsx) and SQLite files.".into(),
    )
}

/// DuckDB errors can be long; the first line is the useful part.
fn short(message: &str) -> String {
    message
        .lines()
        .next()
        .unwrap_or(message)
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_names_cannot_escape() {
        assert_eq!(safe_file_name("orders.csv"), "orders.csv");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\a\\sales.xlsx"), "sales.xlsx");
        assert_eq!(safe_file_name("..hidden.csv"), "hidden.csv");
        assert_eq!(safe_file_name("..."), "upload");
        assert_eq!(safe_file_name("a\u{0}b.csv"), "ab.csv");
    }

    #[test]
    fn references() {
        let id = Uuid::nil();
        assert_eq!(
            table_reference(
                SourceKind::File,
                id,
                "s_00000000000000000000000000000000",
                "orders"
            ),
            "\"project\".\"s_00000000000000000000000000000000\".\"orders\""
        );
        assert!(table_reference(SourceKind::Postgres, id, "public", "t").starts_with("\"src_"));
    }
}
