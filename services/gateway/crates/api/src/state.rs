use std::str::FromStr as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lattice_aibridge::{AiBridgeError, AiClient};
use lattice_cache::ResultCache;
use lattice_connectors::MetadataDb;
use lattice_engine::{Engine, EngineConfig};
use lattice_security::{SecretBox, SessionStore};
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use openidconnect::reqwest;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;

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
    #[error("invalid LATTICE_SECRET_KEY: {0}")]
    SecretKey(#[from] lattice_security::CryptoError),
    #[error("invalid object storage configuration: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("invalid DATABASE_URL: {0}")]
    DatabaseUrl(#[from] sqlx::Error),
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
    pub engine: Arc<Engine>,
    pub cache: ResultCache,
    /// Encrypts source credentials; `None` when LATTICE_SECRET_KEY is unset.
    pub secrets: Option<SecretBox>,
    /// Durable copies of uploads and extracts; `None` when object storage isn't configured.
    pub store: Option<Arc<dyn ObjectStore>>,
    pub metadata_db: MetadataDb,
    pub started_at: Instant,
}

impl AppState {
    /// Never blocks on Redis, the AI service or object storage: all connect lazily, so the
    /// gateway can start and serve while any of them is still booting.
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

        let e = &config.engine;
        let engine = Engine::new(EngineConfig {
            data_dir: config.data_dir.clone(),
            extension_dir: e.extension_dir.clone(),
            memory_limit: e.memory_limit.clone(),
            threads: e.threads,
            pool_size: e.pool_size.max(1),
            max_open_projects: e.max_open_projects.max(1),
            query_timeout: Duration::from_secs(e.query_timeout_secs.max(1)),
        });
        let cache = ResultCache::new(
            redis.clone(),
            Duration::from_secs(e.cache_ttl_secs),
            e.cache_max_bytes,
        );
        let secrets = config
            .secret_key
            .as_deref()
            .map(SecretBox::from_base64_key)
            .transpose()?;
        let store: Option<Arc<dyn ObjectStore>> = match (
            &config.s3_endpoint,
            &config.s3_access_key,
            &config.s3_secret_key,
        ) {
            (Some(endpoint), Some(key), Some(secret)) => Some(Arc::new(
                AmazonS3Builder::new()
                    .with_endpoint(endpoint)
                    .with_bucket_name(&config.s3_bucket)
                    .with_region(&config.s3_region)
                    .with_access_key_id(key)
                    .with_secret_access_key(secret)
                    .with_allow_http(endpoint.starts_with("http://"))
                    .with_virtual_hosted_style_request(false)
                    .build()?,
            )),
            _ => None,
        };
        let pg = PgConnectOptions::from_str(&config.database_url)?;
        let metadata_db = MetadataDb {
            host: pg.get_host().to_owned(),
            port: pg.get_port(),
            database: pg.get_database().unwrap_or("postgres").to_owned(),
        };

        Ok(Self {
            sessions: SessionStore::new(pool.clone()),
            config: Arc::new(config),
            pool,
            redis,
            ai,
            oidc: Arc::new(oidc),
            http,
            engine: Arc::new(engine),
            cache,
            secrets,
            store,
            metadata_db,
            started_at: Instant::now(),
        })
    }
}
