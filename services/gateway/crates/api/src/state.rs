use std::sync::Arc;
use std::time::{Duration, Instant};

use lattice_aibridge::{AiBridgeError, AiClient};
use lattice_security::SessionStore;
use openidconnect::reqwest;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use sqlx::PgPool;

use crate::auth::oidc::OidcRegistry;
use crate::config::Config;

pub const GATEWAY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("invalid Redis URL: {0}")]
    Redis(#[from] redis::RedisError),
    #[error(transparent)]
    AiBridge(#[from] AiBridgeError),
    #[error("failed to build HTTP client: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid OIDC configuration: {0}")]
    Oidc(String),
}

/// Shared by every request. Cloning is cheap (pools and clients are reference-counted).
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub sessions: SessionStore,
    pub redis: ConnectionManager,
    pub ai: AiClient,
    pub oidc: Arc<OidcRegistry>,
    /// Outbound HTTP (OIDC discovery and token exchange, object-storage health).
    pub http: reqwest::Client,
    pub started_at: Instant,
}

impl AppState {
    /// Never blocks on Redis or the AI service: both connect lazily, so the gateway can
    /// start and serve while either is still booting.
    pub fn new(config: Config, pool: PgPool) -> Result<Self, StartupError> {
        let redis = ConnectionManager::new_lazy_with_config(
            redis::Client::open(config.redis_url.as_str())?,
            ConnectionManagerConfig::new()
                .set_connection_timeout(Some(Duration::from_secs(1)))
                .set_response_timeout(Some(Duration::from_secs(1)))
                .set_number_of_retries(2),
        )?;
        let ai = AiClient::connect_lazy(&config.ai_grpc_url)?;
        let http = reqwest::Client::builder()
            // OIDC token endpoints must not be followed through redirects (SSRF hardening).
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        let oidc = OidcRegistry::new(&config).map_err(StartupError::Oidc)?;
        Ok(Self {
            sessions: SessionStore::new(pool.clone()),
            config: Arc::new(config),
            pool,
            redis,
            ai,
            oidc: Arc::new(oidc),
            http,
            started_at: Instant::now(),
        })
    }
}
