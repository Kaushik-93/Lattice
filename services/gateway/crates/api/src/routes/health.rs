use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::dto::{DependencyCheck, ReadyReport, ReadyStatus};
use crate::state::{AppState, GATEWAY_VERSION};

const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// Liveness: no I/O at all, so it measures pure gateway overhead and never flaps when a
/// dependency does.
pub async fn healthz() -> &'static str {
    "ok"
}

/// Readiness: probes every dependency concurrently (bounded by a 1 s timeout each).
pub async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<ReadyReport>) {
    let pool = state.pool.clone();
    let mut redis = state.redis.clone();
    let ai = state.ai.clone();

    let (postgres, redis, ai, object_storage) = tokio::join!(
        probe("postgres", true, async move {
            sqlx::query_scalar!("SELECT 1 AS one")
                .fetch_one(&pool)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        }),
        probe("redis", true, async move {
            redis::cmd("PING")
                .query_async::<String>(&mut redis)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        }),
        probe("ai", false, async move {
            ai.check_health().await.map_err(|e| e.to_string())
        }),
        probe_object_storage(&state),
    );

    let mut checks = vec![postgres, redis, ai];
    checks.extend(object_storage);
    let status = if checks.iter().any(|c| c.required && !c.ok) {
        ReadyStatus::Unavailable
    } else if checks.iter().any(|c| !c.ok) {
        ReadyStatus::Degraded
    } else {
        ReadyStatus::Ready
    };
    let code = match status {
        ReadyStatus::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ReadyStatus::Ready | ReadyStatus::Degraded => StatusCode::OK,
    };
    (
        code,
        Json(ReadyReport {
            status,
            version: GATEWAY_VERSION.to_owned(),
            uptime_s: state.started_at.elapsed().as_secs(),
            checks,
        }),
    )
}

async fn probe_object_storage(state: &AppState) -> Option<DependencyCheck> {
    let endpoint = state.config.s3_endpoint.as_deref()?;
    let url = format!("{endpoint}/health");
    let http = state.http.clone();
    Some(
        probe("object_storage", false, async move {
            let response = http.get(url).send().await.map_err(|e| e.to_string())?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(format!("HTTP {}", response.status()))
            }
        })
        .await,
    )
}

async fn probe(
    name: &str,
    required: bool,
    check: impl Future<Output = Result<(), String>>,
) -> DependencyCheck {
    let started = Instant::now();
    let (ok, detail) = match tokio::time::timeout(CHECK_TIMEOUT, check).await {
        Ok(Ok(())) => (true, None),
        Ok(Err(err)) => (false, Some(err)),
        Err(_) => (
            false,
            Some(format!("timed out after {}s", CHECK_TIMEOUT.as_secs())),
        ),
    };
    DependencyCheck {
        name: name.to_owned(),
        ok,
        required,
        latency_ms: (started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0,
        detail,
    }
}
