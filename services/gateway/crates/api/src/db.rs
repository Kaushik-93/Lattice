use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions as _, Connection as _, Executor as _, PgConnection, PgPool};

pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Every pooled connection loads Apache AGE so Cypher queries work on any of them.
/// `$libdir/plugins/age` (not plain `age`) also works for non-superuser roles; see
/// infra/docker/postgres/Dockerfile.
const AGE_SESSION_SETUP: &str =
    r#"LOAD '$libdir/plugins/age'; SET search_path = ag_catalog, "$user", public;"#;

pub async fn connect_pool(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    let options: PgConnectOptions = url.parse()?;
    PgPoolOptions::new()
        .max_connections(max_connections)
        // Keep a couple of warm connections so the first request after idle doesn't pay a TCP+auth handshake.
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute(AGE_SESSION_SETUP).await?;
                Ok(())
            })
        })
        .connect_with(options.disable_statement_logging())
        .await
}

/// Runs on a dedicated connection with the default search_path: with `ag_catalog` first
/// (as on pooled connections), unqualified CREATE TABLE would land in the AGE schema.
pub async fn run_migrations(url: &str) -> Result<(), MigrateError> {
    let mut conn = PgConnection::connect(url).await?;
    MIGRATOR.run(&mut conn).await?;
    conn.close().await?;
    Ok(())
}
