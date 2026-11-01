//! The unified catalog: each project's sources, their tables and columns, and column profiles.
//! Postgres is the source of truth; DuckDB holds the data (docs/adr/0004).

use chrono::{DateTime, Utc};
use lattice_connectors::{SourceKind, TableMode};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("corrupt catalog row: {0}")]
    Corrupt(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceStatus {
    Syncing,
    Ready,
    Error,
}

impl SourceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            SourceStatus::Syncing => "syncing",
            SourceStatus::Ready => "ready",
            SourceStatus::Error => "error",
        }
    }

    fn parse(s: &str) -> Result<Self, CatalogError> {
        match s {
            "syncing" => Ok(Self::Syncing),
            "ready" => Ok(Self::Ready),
            "error" => Ok(Self::Error),
            other => Err(CatalogError::Corrupt(format!("status {other:?}"))),
        }
    }
}

/// A row on the Sources screen.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SourceSummary {
    pub id: Uuid,
    pub name: String,
    pub kind: SourceKind,
    pub status: SourceStatus,
    pub status_detail: Option<String>,
    /// e.g. "orders.csv · 2.1 MB" or "reader@db.internal:5432/acme".
    pub detail: String,
    #[ts(type = "number")]
    pub table_count: i64,
    #[ts(type = "number | null")]
    pub row_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SourceDetail {
    pub source: SourceSummary,
    pub tables: Vec<SourceTable>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SourceTable {
    pub id: Uuid,
    pub schema_name: String,
    pub name: String,
    pub mode: TableMode,
    #[ts(type = "number | null")]
    pub row_count: Option<i64>,
    pub profiled_at: Option<DateTime<Utc>>,
    pub columns: Vec<SourceColumn>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SourceColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub profile: Option<ColumnStats>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ColumnStats {
    pub min: Option<String>,
    pub max: Option<String>,
    #[ts(type = "number | null")]
    pub approx_distinct: Option<i64>,
    pub null_fraction: Option<f64>,
    pub avg: Option<String>,
    pub samples: Vec<String>,
}

pub struct NewSource<'a> {
    pub project_id: Uuid,
    pub kind: SourceKind,
    pub name: &'a str,
    pub config: serde_json::Value,
    pub created_by: Uuid,
}

/// Introspection result to store.
pub struct TableSpec {
    pub schema: String,
    pub name: String,
    pub row_count: Option<i64>,
    pub columns: Vec<ColumnSpec>,
}

pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

/// Everything needed to (re)attach or reload a source.
#[derive(Debug, Clone)]
pub struct SourceRecord {
    pub id: Uuid,
    pub kind: SourceKind,
    pub name: String,
    pub config: serde_json::Value,
    pub secret: Option<Vec<u8>>,
}

/// A table resolved for querying.
#[derive(Debug, Clone)]
pub struct ResolvedTable {
    pub source_id: Uuid,
    pub kind: SourceKind,
    pub schema_name: String,
    pub table_name: String,
    pub data_version: i64,
    pub columns: Vec<(String, String)>,
}

fn kind(s: &str) -> Result<SourceKind, CatalogError> {
    SourceKind::parse(s).ok_or_else(|| CatalogError::Corrupt(format!("kind {s:?}")))
}

fn mode(s: &str) -> Result<TableMode, CatalogError> {
    match s {
        "live" => Ok(TableMode::Live),
        "extract" => Ok(TableMode::Extract),
        other => Err(CatalogError::Corrupt(format!("mode {other:?}"))),
    }
}

const fn mode_str(m: TableMode) -> &'static str {
    match m {
        TableMode::Live => "live",
        TableMode::Extract => "extract",
    }
}

pub async fn insert_source(pool: &PgPool, new: NewSource<'_>) -> Result<Uuid, CatalogError> {
    Ok(sqlx::query_scalar!(
        "INSERT INTO sources (project_id, kind, name, config, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        new.project_id,
        new.kind.as_str(),
        new.name,
        new.config,
        new.created_by,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn set_secret(pool: &PgPool, source_id: Uuid, sealed: &[u8]) -> Result<(), CatalogError> {
    sqlx::query!(
        "UPDATE sources SET secret = $2 WHERE id = $1",
        source_id,
        sealed
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_config(
    pool: &PgPool,
    source_id: Uuid,
    config: &serde_json::Value,
) -> Result<(), CatalogError> {
    sqlx::query!(
        "UPDATE sources SET config = $2 WHERE id = $1",
        source_id,
        config
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Records a sync outcome. A successful sync bumps the data version, so cached results of the
/// old data are never served again.
pub async fn set_status(
    pool: &PgPool,
    source_id: Uuid,
    status: SourceStatus,
    detail: Option<&str>,
) -> Result<(), CatalogError> {
    sqlx::query!(
        "UPDATE sources SET status = $2, status_detail = $3,
             synced_at = CASE WHEN $2 = 'ready' THEN now() ELSE synced_at END,
             data_version = CASE WHEN $2 = 'ready' THEN data_version + 1 ELSE data_version END
         WHERE id = $1",
        source_id,
        status.as_str(),
        detail,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Upserts the source's tables and replaces their columns; tables no longer present are
/// removed. Returns table ids in the order given.
pub async fn sync_tables(
    pool: &PgPool,
    source_id: Uuid,
    table_mode: TableMode,
    tables: &[TableSpec],
) -> Result<Vec<Uuid>, CatalogError> {
    let mut tx = pool.begin().await?;
    let mut ids = Vec::with_capacity(tables.len());
    for t in tables {
        let id = sqlx::query_scalar!(
            "INSERT INTO source_tables (source_id, schema_name, table_name, mode, row_count)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (source_id, schema_name, table_name)
             DO UPDATE SET mode = EXCLUDED.mode, row_count = COALESCE(EXCLUDED.row_count, source_tables.row_count)
             RETURNING id",
            source_id,
            t.schema,
            t.name,
            mode_str(table_mode),
            t.row_count,
        )
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM source_columns WHERE table_id = $1", id)
            .execute(&mut *tx)
            .await?;
        let ordinals: Vec<i32> = (0..t.columns.len())
            .map(|i| i32::try_from(i).unwrap_or(i32::MAX))
            .collect();
        let names: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
        let types: Vec<String> = t.columns.iter().map(|c| c.data_type.clone()).collect();
        let nullables: Vec<bool> = t.columns.iter().map(|c| c.nullable).collect();
        sqlx::query!(
            "INSERT INTO source_columns (table_id, ordinal, name, data_type, nullable)
             SELECT $1, * FROM UNNEST($2::int[], $3::text[], $4::text[], $5::bool[])",
            id,
            &ordinals,
            &names,
            &types,
            &nullables,
        )
        .execute(&mut *tx)
        .await?;
        ids.push(id);
    }
    sqlx::query!(
        "DELETE FROM source_tables WHERE source_id = $1 AND NOT (id = ANY($2))",
        source_id,
        &ids,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ids)
}

pub async fn set_profile(
    pool: &PgPool,
    table_id: Uuid,
    row_count: Option<i64>,
    stats: &[(String, ColumnStats)],
) -> Result<(), CatalogError> {
    let mut tx = pool.begin().await?;
    for (column, s) in stats {
        let profile = serde_json::to_value(s).map_err(|e| CatalogError::Corrupt(e.to_string()))?;
        sqlx::query!(
            "UPDATE source_columns SET profile = $3 WHERE table_id = $1 AND name = $2",
            table_id,
            column,
            profile,
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        "UPDATE source_tables SET row_count = COALESCE($2, row_count), profiled_at = now() WHERE id = $1",
        table_id,
        row_count,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn list_sources(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<SourceSummary>, CatalogError> {
    sqlx::query!(
        r#"SELECT s.id, s.name, s.kind, s.status, s.status_detail, s.config, s.created_at, s.synced_at,
                  (SELECT count(*) FROM source_tables t WHERE t.source_id = s.id) AS "table_count!",
                  (SELECT sum(t.row_count)::bigint FROM source_tables t WHERE t.source_id = s.id) AS row_count
           FROM sources s WHERE s.project_id = $1 ORDER BY s.created_at"#,
        project_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        let kind = kind(&r.kind)?;
        Ok(SourceSummary {
            detail: describe(kind, &r.config),
            id: r.id,
            name: r.name,
            kind,
            status: SourceStatus::parse(&r.status)?,
            status_detail: r.status_detail,
            table_count: r.table_count,
            row_count: r.row_count,
            created_at: r.created_at,
            synced_at: r.synced_at,
        })
    })
    .collect()
}

pub async fn source_detail(
    pool: &PgPool,
    project_id: Uuid,
    source_id: Uuid,
) -> Result<Option<SourceDetail>, CatalogError> {
    let Some(source) = list_sources(pool, project_id)
        .await?
        .into_iter()
        .find(|s| s.id == source_id)
    else {
        return Ok(None);
    };
    let tables = sqlx::query!(
        "SELECT id, schema_name, table_name, mode, row_count, profiled_at
         FROM source_tables WHERE source_id = $1 ORDER BY schema_name, table_name",
        source_id,
    )
    .fetch_all(pool)
    .await?;
    let columns = sqlx::query!(
        "SELECT c.table_id, c.name, c.data_type, c.nullable, c.profile
         FROM source_columns c JOIN source_tables t ON t.id = c.table_id
         WHERE t.source_id = $1 ORDER BY c.table_id, c.ordinal",
        source_id,
    )
    .fetch_all(pool)
    .await?;
    let tables = tables
        .into_iter()
        .map(|t| {
            Ok(SourceTable {
                columns: columns
                    .iter()
                    .filter(|c| c.table_id == t.id)
                    .map(|c| SourceColumn {
                        name: c.name.clone(),
                        data_type: c.data_type.clone(),
                        nullable: c.nullable,
                        profile: c
                            .profile
                            .clone()
                            .and_then(|p| serde_json::from_value(p).ok()),
                    })
                    .collect(),
                id: t.id,
                schema_name: t.schema_name,
                name: t.table_name,
                mode: mode(&t.mode)?,
                row_count: t.row_count,
                profiled_at: t.profiled_at,
            })
        })
        .collect::<Result<Vec<_>, CatalogError>>()?;
    Ok(Some(SourceDetail { source, tables }))
}

pub async fn source_record(
    pool: &PgPool,
    project_id: Uuid,
    source_id: Uuid,
) -> Result<Option<SourceRecord>, CatalogError> {
    sqlx::query!(
        "SELECT id, kind, name, config, secret FROM sources WHERE project_id = $1 AND id = $2",
        project_id,
        source_id,
    )
    .fetch_optional(pool)
    .await?
    .map(|r| {
        Ok(SourceRecord {
            id: r.id,
            kind: kind(&r.kind)?,
            name: r.name,
            config: r.config,
            secret: r.secret,
        })
    })
    .transpose()
}

/// Sources attached live when the project database opens.
pub async fn live_sources(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<SourceRecord>, CatalogError> {
    sqlx::query!(
        "SELECT id, kind, name, config, secret FROM sources
         WHERE project_id = $1 AND kind IN ('sqlite', 'postgres', 'mysql') ORDER BY created_at",
        project_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        Ok(SourceRecord {
            id: r.id,
            kind: kind(&r.kind)?,
            name: r.name,
            config: r.config,
            secret: r.secret,
        })
    })
    .collect()
}

/// Resolves a table for querying, scoped to the project (other projects' tables don't exist).
pub async fn resolve_table(
    pool: &PgPool,
    project_id: Uuid,
    table_id: Uuid,
) -> Result<Option<ResolvedTable>, CatalogError> {
    let Some(t) = sqlx::query!(
        "SELECT t.source_id, s.kind, t.schema_name, t.table_name, s.data_version
         FROM source_tables t JOIN sources s ON s.id = t.source_id
         WHERE t.id = $1 AND s.project_id = $2",
        table_id,
        project_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let columns = sqlx::query!(
        "SELECT name, data_type FROM source_columns WHERE table_id = $1 ORDER BY ordinal",
        table_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|c| (c.name, c.data_type))
    .collect();
    Ok(Some(ResolvedTable {
        source_id: t.source_id,
        kind: kind(&t.kind)?,
        schema_name: t.schema_name,
        table_name: t.table_name,
        data_version: t.data_version,
        columns,
    }))
}

pub async fn delete_source(
    pool: &PgPool,
    project_id: Uuid,
    source_id: Uuid,
) -> Result<bool, CatalogError> {
    let done = sqlx::query!(
        "DELETE FROM sources WHERE project_id = $1 AND id = $2",
        project_id,
        source_id,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// One-line description of a source for lists.
pub fn describe(kind: SourceKind, config: &serde_json::Value) -> String {
    let text = |k: &str| {
        config
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    match kind {
        SourceKind::File | SourceKind::Sqlite => {
            let size = config
                .get("size_bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            format!("{} · {}", text("file_name"), human_bytes(size))
        }
        SourceKind::Postgres | SourceKind::Mysql => {
            let port = config
                .get("port")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            format!(
                "{}@{}:{port}/{}",
                text("user"),
                text("host"),
                text("database")
            )
        }
        SourceKind::Demo => {
            let rows = config
                .get("rows")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            format!("generated · {} orders", human_count(rows))
        }
    }
}

fn human_count(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{}M", (n as f64 / 1e6 * 100.0).round() / 100.0),
        n if n >= 1_000 => format!("{}K", (n as f64 / 1e3 * 10.0).round() / 10.0),
        n => n.to_string(),
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn descriptions() {
        assert_eq!(
            describe(
                SourceKind::File,
                &json!({"file_name": "orders.csv", "size_bytes": 2_100_000})
            ),
            "orders.csv · 2.1 MB"
        );
        assert_eq!(
            describe(
                SourceKind::Postgres,
                &json!({"host": "db", "port": 5432, "database": "acme", "user": "reader"})
            ),
            "reader@db:5432/acme"
        );
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(
            describe(SourceKind::Demo, &json!({"rows": 1_000_000})),
            "generated · 1M orders"
        );
        assert_eq!(
            describe(SourceKind::Demo, &json!({"rows": 50_000_000})),
            "generated · 50M orders"
        );
    }
}
