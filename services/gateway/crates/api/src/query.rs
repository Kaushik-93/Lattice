//! One query, end to end: resolve the table, compile, consult the result cache, execute,
//! and frame the Arrow result for the browser — timing every stage (spec §3 rule 8).

use std::time::{Duration, Instant};

use lattice_cache::CacheKey;
use lattice_catalog as catalog;
use lattice_engine::{EngineError, ipc};
use lattice_planner::{ColumnInfo, TableQuery, TableSchema, compile};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::dto::{QueryTimings, ResultHeader};
use crate::sources::{project_db, table_reference};
use crate::state::AppState;

pub struct QueryOutcome {
    pub header: ResultHeader,
    pub ipc: Vec<u8>,
}

#[derive(Debug)]
pub enum QueryFailure {
    Cancelled,
    Failed { code: &'static str, message: String },
}

impl QueryFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        QueryFailure::Failed {
            code,
            message: message.into(),
        }
    }
}

/// What the cache stores next to the IPC bytes.
#[derive(Serialize, Deserialize)]
struct CachedMeta {
    rows: u32,
    truncated: bool,
}

pub async fn run(
    state: &AppState,
    project_id: Uuid,
    batch_id: u32,
    query_id: String,
    query: TableQuery,
    cancel: CancellationToken,
) -> Result<QueryOutcome, QueryFailure> {
    let started = Instant::now();
    let mut timings = QueryTimings::default();

    let table = catalog::resolve_table(&state.pool, project_id, query.table_id)
        .await
        .map_err(|e| QueryFailure::new("internal", e.to_string()))?
        .ok_or_else(|| QueryFailure::new("not_found", "That table isn't in this project."))?;
    let schema = TableSchema {
        reference: table_reference(
            table.kind,
            table.source_id,
            &table.schema_name,
            &table.table_name,
        ),
        columns: table
            .columns
            .iter()
            .map(|(name, data_type)| ColumnInfo {
                name: name.clone(),
                data_type: data_type.clone(),
            })
            .collect(),
    };

    let t = Instant::now();
    let compiled =
        compile(&query, &schema).map_err(|e| QueryFailure::new("invalid_query", e.to_string()))?;
    timings.compile_ms = ms(t.elapsed());

    let params = format!("{:?}", compiled.params);
    let key = CacheKey {
        project_id,
        sql: &compiled.sql,
        params: &params,
        data_version: table.data_version,
        logic_version: 0, // phase 3: business-logic version
        rls_context: "",  // phase 5: row-level-security context
    }
    .render();

    let t = Instant::now();
    let hit = state.cache.get(&key).await.and_then(|bytes| unpack(&bytes));
    timings.cache_ms = ms(t.elapsed());
    if let Some((meta, ipc)) = hit {
        timings.total_ms = ms(started.elapsed());
        return Ok(QueryOutcome {
            header: ResultHeader {
                batch_id,
                query_id,
                rows: meta.rows,
                truncated: meta.truncated,
                cached: true,
                columns: compiled.columns,
                timings,
            },
            ipc,
        });
    }

    let db = project_db(state, project_id)
        .await
        .map_err(|e| QueryFailure::new("unavailable", e.to_string()))?;
    let out = db
        .query(compiled.sql, compiled.params, &cancel)
        .await
        .map_err(|e| match e {
            EngineError::Cancelled => QueryFailure::Cancelled,
            EngineError::Timeout(_) => QueryFailure::new("timeout", e.to_string()),
            other => QueryFailure::new("query_failed", other.to_string()),
        })?;
    timings.queue_ms = ms(out.queued);
    timings.execute_ms = ms(out.executed);

    let limit = compiled.row_limit as usize;
    let truncated = out.rows > limit;
    let rows = u32::try_from(out.rows.min(limit)).unwrap_or(u32::MAX);
    let t = Instant::now();
    let encoded = ipc::for_browser(&out.schema, ipc::truncate(out.batches, limit))
        .and_then(|(schema, batches)| ipc::encode(&schema, &batches));
    let ipc = encoded.map_err(|e| QueryFailure::new("internal", e.to_string()))?;
    timings.serialize_ms = ms(t.elapsed());

    // Populate the cache off the response path.
    let cache = state.cache.clone();
    let packed = pack(&CachedMeta { rows, truncated }, &ipc);
    tokio::spawn(async move { cache.put(&key, &packed).await });

    timings.total_ms = ms(started.elapsed());
    Ok(QueryOutcome {
        header: ResultHeader {
            batch_id,
            query_id,
            rows,
            truncated,
            cached: false,
            columns: compiled.columns,
            timings,
        },
        ipc,
    })
}

/// `[u32 LE header length][header JSON][Arrow IPC stream]` — the binary WebSocket frame.
pub fn frame(header: &ResultHeader, ipc: &[u8]) -> Vec<u8> {
    let header = serde_json::to_vec(header).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + header.len() + ipc.len());
    out.extend_from_slice(
        &u32::try_from(header.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&header);
    out.extend_from_slice(ipc);
    out
}

/// Splits a frame back into header and IPC bytes (tests, benchmarks, clients written in Rust).
pub fn unframe(frame: &[u8]) -> Option<(ResultHeader, &[u8])> {
    let len = u32::from_le_bytes(frame.get(..4)?.try_into().ok()?) as usize;
    let header = serde_json::from_slice(frame.get(4..4 + len)?).ok()?;
    Some((header, frame.get(4 + len..)?))
}

fn pack(meta: &CachedMeta, ipc: &[u8]) -> Vec<u8> {
    let meta = serde_json::to_vec(meta).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + meta.len() + ipc.len());
    out.extend_from_slice(&u32::try_from(meta.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&meta);
    out.extend_from_slice(ipc);
    out
}

fn unpack(bytes: &[u8]) -> Option<(CachedMeta, Vec<u8>)> {
    let len = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let meta = serde_json::from_slice(bytes.get(4..4 + len)?).ok()?;
    Some((meta, bytes.get(4 + len..)?.to_vec()))
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e6).round() / 1e3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let header = ResultHeader {
            batch_id: 3,
            query_id: "q1".into(),
            rows: 2,
            truncated: false,
            cached: true,
            columns: vec![],
            timings: QueryTimings::default(),
        };
        let bytes = frame(&header, b"ARROW");
        let (decoded, ipc) = unframe(&bytes).unwrap();
        assert_eq!(
            (decoded.batch_id, decoded.query_id.as_str(), ipc),
            (3, "q1", b"ARROW".as_slice())
        );
        assert!(unframe(&bytes[..3]).is_none());
    }

    #[test]
    fn cache_entries_round_trip() {
        let packed = pack(
            &CachedMeta {
                rows: 7,
                truncated: true,
            },
            b"IPC",
        );
        let (meta, ipc) = unpack(&packed).unwrap();
        assert_eq!(
            (meta.rows, meta.truncated, ipc.as_slice()),
            (7, true, b"IPC".as_slice())
        );
    }
}
