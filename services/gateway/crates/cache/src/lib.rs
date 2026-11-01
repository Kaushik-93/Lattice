//! Hot result cache in Redis (spec §3 rule 4).
//!
//! Keys hash everything that determines a result: the compiled SQL and its parameters, the
//! source's data version, the business-logic version (phase 3) and the row-level-security
//! context (phase 5). A change to any of them is a different key, so nothing is ever
//! invalidated by hand; stale entries simply age out.
//!
//! The cache is best-effort: a Redis error is logged and treated as a miss, never as a failed
//! query.

use std::fmt::Write as _;
use std::time::Duration;

use redis::aio::ConnectionManager;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

/// Redis round trips are sub-millisecond; anything slower means trouble, and the query runs anyway.
const REDIS_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub struct ResultCache {
    redis: ConnectionManager,
    ttl: Duration,
    max_bytes: usize,
}

/// Inputs that determine a query result.
pub struct CacheKey<'a> {
    pub project_id: Uuid,
    pub sql: &'a str,
    /// Bound parameters, rendered unambiguously (e.g. their `Debug` form).
    pub params: &'a str,
    /// Every table read, with its data version.
    pub data_versions: &'a [(Uuid, i64)],
    /// Every business-logic definition used, as `note_id@version` (empty for raw queries).
    pub logic: &'a str,
    pub rls_context: &'a str,
}

impl CacheKey<'_> {
    pub fn render(&self) -> String {
        let mut h = Sha256::new();
        let mut versions = Vec::with_capacity(self.data_versions.len() * 24);
        for (table, version) in self.data_versions {
            versions.extend_from_slice(table.as_bytes());
            versions.extend_from_slice(&version.to_le_bytes());
        }
        // Length-prefix every part so no two different inputs can collide by concatenation.
        for part in [
            self.project_id.as_bytes().as_slice(),
            self.sql.as_bytes(),
            self.params.as_bytes(),
            &versions,
            self.logic.as_bytes(),
            self.rls_context.as_bytes(),
        ] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part);
        }
        let digest = h.finalize();
        let mut key = String::with_capacity(70);
        key.push_str("rc:v2:");
        for byte in digest {
            let _ = write!(key, "{byte:02x}");
        }
        key
    }
}

impl ResultCache {
    pub fn new(redis: ConnectionManager, ttl: Duration, max_bytes: usize) -> Self {
        Self {
            redis,
            ttl,
            max_bytes,
        }
    }

    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let mut conn = self.redis.clone();
        let mut cmd = redis::cmd("GET");
        cmd.arg(key);
        match tokio::time::timeout(REDIS_TIMEOUT, cmd.query_async::<Option<Vec<u8>>>(&mut conn))
            .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "result cache read failed");
                None
            }
            Err(_) => {
                tracing::warn!("result cache read timed out");
                None
            }
        }
    }

    /// Stores a value unless it exceeds the size cap (big results aren't worth the memory).
    pub async fn put(&self, key: &str, value: &[u8]) {
        if value.len() > self.max_bytes {
            return;
        }
        let mut conn = self.redis.clone();
        let mut cmd = redis::cmd("SET");
        cmd.arg(key)
            .arg(value)
            .arg("EX")
            .arg(self.ttl.as_secs().max(1));
        match tokio::time::timeout(REDIS_TIMEOUT, cmd.query_async::<()>(&mut conn)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(error = %err, "result cache write failed"),
            Err(_) => tracing::warn!("result cache write timed out"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sql: &str, params: &str, version: i64) -> String {
        logic_key(sql, params, version, "")
    }

    fn logic_key(sql: &str, params: &str, version: i64, logic: &str) -> String {
        CacheKey {
            project_id: Uuid::nil(),
            sql,
            params,
            data_versions: &[(Uuid::nil(), version)],
            logic,
            rls_context: "",
        }
        .render()
    }

    #[test]
    fn stable_and_sensitive_to_every_input() {
        let base = key("SELECT 1", "[]", 1);
        assert_eq!(base, key("SELECT 1", "[]", 1));
        assert!(base.starts_with("rc:v2:") && base.len() == 6 + 64);
        assert_ne!(base, key("SELECT 2", "[]", 1));
        assert_ne!(base, key("SELECT 1", "[1]", 1));
        assert_ne!(base, key("SELECT 1", "[]", 2), "new data version → new key");
        assert_ne!(
            logic_key("SELECT 1", "[]", 1, "n@1"),
            logic_key("SELECT 1", "[]", 1, "n@2"),
            "new definition version → new key"
        );
    }

    #[test]
    fn concatenation_cannot_collide() {
        assert_ne!(key("SELECT 1[", "]", 1), key("SELECT 1", "[]", 1));
    }
}
