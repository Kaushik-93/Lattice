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
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
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

        Ok(Self {
            bind_addr: parse(&get, "BIND_ADDR", "0.0.0.0:8080")?,
            tools_addr: parse(&get, "TOOLS_GRPC_ADDR", "0.0.0.0:50052")?,
            database_url: required("DATABASE_URL")?,
            database_max_connections: parse(&get, "DATABASE_MAX_CONNECTIONS", "16")?,
            redis_url: required("REDIS_URL")?,
            ai_grpc_url: get("AI_GRPC_URL").unwrap_or_else(|| "http://127.0.0.1:50051".into()),
            s3_endpoint: get("S3_ENDPOINT").map(|s| s.trim_end_matches('/').to_owned()),
            data_dir: get("DATA_DIR").unwrap_or_else(|| "./data".into()).into(),
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
