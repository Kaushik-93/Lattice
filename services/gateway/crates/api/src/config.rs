use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {reason}")]
    Invalid { name: String, reason: String },
}

#[derive(Debug, Clone)]
pub struct OidcProviderConfig {
    /// URL-safe id used in routes (`/api/auth/oidc/{id}/start`), e.g. `google`.
    pub id: String,
    pub label: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub tools_addr: SocketAddr,
    pub database_url: String,
    pub database_max_connections: u32,
    pub redis_url: String,
    pub ai_grpc_url: String,
    pub s3_endpoint: Option<String>,
    pub data_dir: PathBuf,
    /// The gateway's externally reachable base URL (OIDC redirect URIs are built from it).
    pub public_url: String,
    /// Origin of the web app: the only origin allowed by CORS and on WebSocket upgrades.
    pub web_origin: String,
    pub cookie_secure: bool,
    pub seed_demo: bool,
    pub log_json: bool,
    pub otlp_endpoint: Option<String>,
    pub oidc_providers: Vec<OidcProviderConfig>,
    /// Base64 AES-256 key that encrypts source credentials. Database sources are refused without it.
    pub secret_key: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub s3_bucket: String,
    pub s3_region: String,
    pub engine: EngineSettings,
    /// Orders in the generated demo dataset.
    pub demo_order_rows: u64,
}

#[derive(Debug, Clone)]
pub struct EngineSettings {
    pub extension_dir: PathBuf,
    pub memory_limit: String,
    pub threads: Option<u32>,
    pub pool_size: usize,
    pub max_open_projects: usize,
    pub query_timeout_secs: u64,
    pub cache_ttl_secs: u64,
    pub cache_max_bytes: usize,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// For commands that need only paths (`install-extensions` at image build time, where no
    /// database or Redis is configured): required variables get placeholders.
    pub fn from_env_or_defaults() -> Self {
        Self::from_lookup(|key| match key {
            "DATABASE_URL" | "REDIS_URL" => {
                std::env::var(key).ok().or_else(|| Some("unused://".into()))
            }
            _ => std::env::var(key).ok(),
        })
        .unwrap_or_else(|_| {
            Self::from_lookup(|key| match key {
                "DATABASE_URL" | "REDIS_URL" => Some("unused://".into()),
                _ => None,
            })
            .unwrap_or_else(|_| unreachable!("defaults always parse"))
        })
    }

    /// Builds the config from any key/value source, so tests don't touch process env.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &str| get(key).filter(|v| !v.trim().is_empty());
        let required = |key: &'static str| get(key).ok_or(ConfigError::Missing(key));

        let oidc_providers = get("OIDC_PROVIDERS")
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(|id| oidc_provider(id, &get))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();

        let data_dir: PathBuf = get("DATA_DIR").unwrap_or_else(|| "./data".into()).into();
        Ok(Self {
            bind_addr: parse(&get, "BIND_ADDR", "0.0.0.0:8080")?,
            tools_addr: parse(&get, "TOOLS_GRPC_ADDR", "0.0.0.0:50052")?,
            database_url: required("DATABASE_URL")?,
            database_max_connections: parse(&get, "DATABASE_MAX_CONNECTIONS", "16")?,
            redis_url: required("REDIS_URL")?,
            ai_grpc_url: get("AI_GRPC_URL").unwrap_or_else(|| "http://127.0.0.1:50051".into()),
            s3_endpoint: get("S3_ENDPOINT").map(|s| s.trim_end_matches('/').to_owned()),
            data_dir: data_dir.clone(),
            public_url: get("PUBLIC_URL")
                .unwrap_or_else(|| "http://localhost:8080".into())
                .trim_end_matches('/')
                .to_owned(),
            web_origin: get("WEB_ORIGIN")
                .unwrap_or_else(|| "http://localhost:3000".into())
                .trim_end_matches('/')
                .to_owned(),
            cookie_secure: parse(&get, "COOKIE_SECURE", "true")?,
            seed_demo: parse(&get, "SEED_DEMO", "false")?,
            log_json: get("LOG_FORMAT").is_some_and(|f| f.eq_ignore_ascii_case("json")),
            otlp_endpoint: get("OTEL_EXPORTER_OTLP_ENDPOINT"),
            oidc_providers,
            secret_key: get("LATTICE_SECRET_KEY"),
            s3_access_key: get("S3_ACCESS_KEY"),
            s3_secret_key: get("S3_SECRET_KEY"),
            s3_bucket: get("S3_BUCKET").unwrap_or_else(|| "lattice".into()),
            s3_region: get("S3_REGION").unwrap_or_else(|| "us-east-1".into()),
            engine: EngineSettings {
                extension_dir: get("DUCKDB_EXTENSION_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| data_dir.join("duckdb-extensions")),
                memory_limit: get("ENGINE_MEMORY_LIMIT").unwrap_or_else(|| "2GB".into()),
                threads: get("ENGINE_THREADS")
                    .map(|t| {
                        t.parse()
                            .map_err(|e: std::num::ParseIntError| ConfigError::Invalid {
                                name: "ENGINE_THREADS".into(),
                                reason: e.to_string(),
                            })
                    })
                    .transpose()?,
                pool_size: parse(&get, "ENGINE_POOL_SIZE", "8")?,
                max_open_projects: parse(&get, "ENGINE_MAX_OPEN_PROJECTS", "16")?,
                query_timeout_secs: parse(&get, "QUERY_TIMEOUT_SECS", "30")?,
                cache_ttl_secs: parse(&get, "RESULT_CACHE_TTL_SECS", "600")?,
                cache_max_bytes: parse(&get, "RESULT_CACHE_MAX_BYTES", "8388608")?,
            },
            demo_order_rows: parse(&get, "DEMO_ORDER_ROWS", "1000000")?,
        })
    }
}

fn parse<T: std::str::FromStr>(
    get: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: &str,
) -> Result<T, ConfigError>
where
    T::Err: std::fmt::Display,
{
    let raw = get(key).unwrap_or_else(|| default.to_owned());
    raw.trim()
        .parse()
        .map_err(|e: T::Err| ConfigError::Invalid {
            name: key.to_owned(),
            reason: e.to_string(),
        })
}

fn oidc_provider(
    id: &str,
    get: &impl Fn(&str) -> Option<String>,
) -> Result<OidcProviderConfig, ConfigError> {
    if !id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(ConfigError::Invalid {
            name: "OIDC_PROVIDERS".into(),
            reason: format!("provider id {id:?} must be lowercase letters, digits or '-'"),
        });
    }
    let prefix = format!("OIDC_{}_", id.to_ascii_uppercase().replace('-', "_"));
    let var = |suffix: &str| get(&format!("{prefix}{suffix}"));
    let missing = |suffix: &str| ConfigError::Invalid {
        name: format!("{prefix}{suffix}"),
        reason: format!("required because OIDC_PROVIDERS lists {id:?}"),
    };
    let default_label = match id {
        "google" => "Continue with Google".to_owned(),
        "microsoft" => "Continue with Microsoft".to_owned(),
        "sso" => "Single sign-on (SSO)".to_owned(),
        other => format!("Continue with {other}"),
    };
    Ok(OidcProviderConfig {
        id: id.to_owned(),
        label: var("LABEL").unwrap_or(default_label),
        issuer: var("ISSUER").ok_or_else(|| missing("ISSUER"))?,
        client_id: var("CLIENT_ID").ok_or_else(|| missing("CLIENT_ID"))?,
        client_secret: var("CLIENT_SECRET"),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        Config::from_lookup(|k| env.get(k).cloned())
    }

    const BASE: [(&str, &str); 2] = [("DATABASE_URL", "postgres://x"), ("REDIS_URL", "redis://x")];

    #[test]
    fn defaults_are_secure() {
        let c = config(&BASE).unwrap();
        assert!(c.cookie_secure);
        assert!(!c.seed_demo);
        assert!(c.oidc_providers.is_empty());
        assert_eq!(c.web_origin, "http://localhost:3000");
    }

    #[test]
    fn requires_database_url() {
        assert!(matches!(
            config(&[("REDIS_URL", "redis://x")]),
            Err(ConfigError::Missing("DATABASE_URL"))
        ));
    }

    #[test]
    fn empty_values_count_as_unset() {
        let mut pairs = BASE.to_vec();
        pairs.push(("OIDC_PROVIDERS", ""));
        pairs.push(("S3_ENDPOINT", " "));
        let c = config(&pairs).unwrap();
        assert!(c.oidc_providers.is_empty());
        assert!(c.s3_endpoint.is_none());
    }

    #[test]
    fn listed_oidc_provider_must_be_fully_configured() {
        let mut pairs = BASE.to_vec();
        pairs.push(("OIDC_PROVIDERS", "google"));
        assert!(config(&pairs).is_err());
        pairs.push(("OIDC_GOOGLE_ISSUER", "https://accounts.google.com"));
        pairs.push(("OIDC_GOOGLE_CLIENT_ID", "abc"));
        let c = config(&pairs).unwrap();
        assert_eq!(c.oidc_providers[0].label, "Continue with Google");
    }

    #[test]
    fn rejects_bad_booleans() {
        let mut pairs = BASE.to_vec();
        pairs.push(("COOKIE_SECURE", "maybe"));
        assert!(config(&pairs).is_err());
    }
}
