//! Lattice data gateway.
//!
//! ```text
//! gateway [serve]      run the HTTP/WebSocket API and the gRPC tool server (default)
//! gateway migrate      apply database migrations and exit
//! gateway seed         create the demo workspace if it doesn't exist and exit
//! gateway healthcheck  exit 0 if the local gateway answers /healthz (container probes)
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context as _, bail};
use lattice_api::{AppState, Config, GATEWAY_VERSION, db, seed};
use lattice_telemetry::TelemetryConfig;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        None | Some("serve") => serve().await,
        Some("migrate") => {
            let config = Config::from_env()?;
            db::run_migrations(&config.database_url).await?;
            println!("migrations applied");
            Ok(())
        }
        Some("seed") => {
            let config = Config::from_env()?;
            let pool = db::connect_pool(&config.database_url, 2).await?;
            let created = seed::seed_demo(&pool).await?;
            println!(
                "{}",
                if created {
                    "demo workspace created"
                } else {
                    "demo workspace already exists"
                }
            );
            Ok(())
        }
        Some("healthcheck") => healthcheck().await,
        Some(other) => {
            bail!("unknown command {other:?}; expected serve, migrate, seed or healthcheck")
        }
    }
}

async fn serve() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let _telemetry = lattice_telemetry::init(&TelemetryConfig {
        service_name: "lattice-gateway".into(),
        json: config.log_json,
        otlp_endpoint: config.otlp_endpoint.clone(),
    })?;

    db::run_migrations(&config.database_url)
        .await
        .context("running migrations")?;
    let pool = db::connect_pool(&config.database_url, config.database_max_connections)
        .await
        .context("connecting to Postgres")?;
    if config.seed_demo && seed::seed_demo(&pool).await? {
        tracing::info!(
            email = seed::DEMO_EMAIL,
            password = seed::DEMO_PASSWORD,
            "seeded demo workspace"
        );
    }

    let bind_addr = config.bind_addr;
    let tools_addr = config.tools_addr;
    let state = AppState::new(config, pool)?;
    spawn_session_purge(state.clone());

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let tools = tokio::spawn(lattice_aibridge::serve_tools(
        tools_addr,
        GATEWAY_VERSION,
        wait_for(shutdown_rx.clone()),
    ));

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("binding {bind_addr}"))?;
    tracing::info!(%bind_addr, %tools_addr, version = GATEWAY_VERSION, "gateway listening");

    let app = lattice_api::router(state).into_make_service_with_connect_info::<SocketAddr>();
    let http = axum::serve(listener, app).with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    http.await.context("HTTP server")?;
    tools.await?.context("gRPC tool server")?;
    tracing::info!("gateway stopped");
    Ok(())
}

fn spawn_session_purge(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match state.sessions.purge_expired().await {
                Ok(0) => {}
                Ok(n) => tracing::info!(purged = n, "removed expired sessions"),
                Err(err) => tracing::warn!(error = %err, "session purge failed"),
            }
        }
    });
}

async fn wait_for(mut rx: watch::Receiver<bool>) {
    // An Err means the sender is gone, which also means: shut down.
    let _ = rx.wait_for(|stop| *stop).await;
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received, draining connections");
}

/// A dependency-free HTTP probe so the distroless image needs no curl.
async fn healthcheck() -> anyhow::Result<()> {
    let port = std::env::var("BIND_ADDR")
        .ok()
        .and_then(|a| a.parse::<SocketAddr>().ok())
        .map_or(8080, |a| a.port());
    let probe = async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut head = [0u8; 12];
        stream.read_exact(&mut head).await?;
        Ok::<_, std::io::Error>(head)
    };
    let head = tokio::time::timeout(Duration::from_secs(2), probe)
        .await
        .context("timed out")??;
    if &head == b"HTTP/1.1 200" {
        Ok(())
    } else {
        bail!("unexpected response: {}", String::from_utf8_lossy(&head))
    }
}
