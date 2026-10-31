//! OpenID Connect sign-in (Google, Microsoft, or any OIDC-compliant SSO provider):
//! authorization-code flow with PKCE, a nonce, and server-side state in Redis.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use lattice_security::identity::{self, OidcProfile};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse as _,
};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use super::password::audit_auth;
use super::start_session;
use crate::config::{Config, OidcProviderConfig};
use crate::dto::AuthProvider;
use crate::error::ApiError;
use crate::state::AppState;

const STATE_COOKIE: &str = "lattice_oidc_state";
const STATE_TTL_SECS: u64 = 600;

type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

pub struct OidcRegistry {
    providers: Vec<Provider>,
}

struct Provider {
    config: OidcProviderConfig,
    redirect_url: RedirectUrl,
    /// Discovered on first use, so an unreachable IdP never blocks gateway startup.
    client: OnceCell<DiscoveredClient>,
}

/// What we remember between redirecting to the IdP and its callback.
#[derive(Serialize, Deserialize)]
struct PendingLogin {
    provider: String,
    nonce: String,
    pkce_verifier: String,
    remember: bool,
}

impl OidcRegistry {
    pub fn new(config: &Config) -> Result<Self, String> {
        let providers = config
            .oidc_providers
            .iter()
            .map(|p| {
                let url = format!("{}/api/auth/oidc/{}/callback", config.public_url, p.id);
                let redirect_url = RedirectUrl::new(url).map_err(|e| e.to_string())?;
                Ok(Provider {
                    config: p.clone(),
                    redirect_url,
                    client: OnceCell::new(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self { providers })
    }

    pub fn list(&self) -> Vec<AuthProvider> {
        self.providers
            .iter()
            .map(|p| AuthProvider {
                id: p.config.id.clone(),
                label: p.config.label.clone(),
            })
            .collect()
    }

    fn get(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.config.id == id)
    }
}

impl Provider {
    async fn client(
        &self,
        http: &openidconnect::reqwest::Client,
    ) -> Result<&DiscoveredClient, ApiError> {
        self.client
            .get_or_try_init(|| async {
                let issuer =
                    IssuerUrl::new(self.config.issuer.clone()).map_err(ApiError::internal)?;
                let metadata = CoreProviderMetadata::discover_async(issuer, http)
                    .await
                    .map_err(ApiError::internal)?;
                Ok::<_, ApiError>(
                    CoreClient::from_provider_metadata(
                        metadata,
                        ClientId::new(self.config.client_id.clone()),
                        self.config.client_secret.clone().map(ClientSecret::new),
                    )
                    .set_redirect_uri(self.redirect_url.clone()),
                )
            })
            .await
    }
}

#[derive(Deserialize)]
pub struct StartQuery {
    #[serde(default)]
    remember: bool,
}

pub async fn start(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Query(query): Query<StartQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), ApiError> {
    let provider = state.oidc.get(&provider_id).ok_or(ApiError::NotFound)?;
    let client = provider.client(&state.http).await?;

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let pending = PendingLogin {
        provider: provider_id,
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
        remember: query.remember,
    };
    let mut redis = state.redis.clone();
    redis::cmd("SET")
        .arg(redis_key(csrf.secret()))
        .arg(serde_json::to_string(&pending)?)
        .arg("EX")
        .arg(STATE_TTL_SECS)
        .query_async::<()>(&mut redis)
        .await?;

    // Binds the callback to this browser: a callback URL forged by someone else (login CSRF)
    // won't carry the matching cookie.
    let cookie = Cookie::build((STATE_COOKIE, csrf.secret().clone()))
        .path("/api/auth/oidc")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::seconds(STATE_TTL_SECS as i64))
        .build();
    Ok((jar.add(cookie), Redirect::to(auth_url.as_str())))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Always answers with a redirect to the web app: `/projects` on success, otherwise
/// `/login?error=sso_failed` (details go to the log, not the URL).
pub async fn callback(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Response {
    let web = state.config.web_origin.clone();
    match finish(&state, &provider_id, query, &headers, jar.clone()).await {
        Ok(jar) => (jar, Redirect::to(&format!("{web}/projects"))).into_response(),
        Err(err) => {
            tracing::warn!(provider = %provider_id, error = %err, source = ?std::error::Error::source(&err), "OIDC sign-in failed");
            let jar = jar.remove(Cookie::build(STATE_COOKIE).path("/api/auth/oidc").build());
            (jar, Redirect::to(&format!("{web}/login?error=sso_failed"))).into_response()
        }
    }
}

async fn finish(
    state: &AppState,
    provider_id: &str,
    query: CallbackQuery,
    headers: &HeaderMap,
    jar: CookieJar,
) -> Result<CookieJar, ApiError> {
    if let Some(error) = query.error {
        return Err(ApiError::BadRequest(format!(
            "identity provider returned {error}"
        )));
    }
    let (Some(code), Some(returned_state)) = (query.code, query.state) else {
        return Err(ApiError::BadRequest(
            "callback without code or state".into(),
        ));
    };
    let cookie_state = jar.get(STATE_COOKIE).map(|c| c.value().to_owned());
    if cookie_state.as_deref() != Some(returned_state.as_str()) {
        return Err(ApiError::BadRequest(
            "state does not match this browser".into(),
        ));
    }

    let mut redis = state.redis.clone();
    let pending: Option<String> = redis::cmd("GETDEL")
        .arg(redis_key(&returned_state))
        .query_async(&mut redis)
        .await?;
    let pending: PendingLogin = serde_json::from_str(
        &pending.ok_or_else(|| ApiError::BadRequest("sign-in attempt expired".into()))?,
    )?;
    if pending.provider != provider_id {
        return Err(ApiError::BadRequest(
            "state belongs to another provider".into(),
        ));
    }

    let provider = state.oidc.get(provider_id).ok_or(ApiError::NotFound)?;
    let client = provider.client(&state.http).await?;
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(ApiError::internal)?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
        .request_async(&state.http)
        .await
        .map_err(ApiError::internal)?;
    let id_token = token
        .id_token()
        .ok_or_else(|| ApiError::internal("identity provider returned no ID token"))?;
    let claims = id_token
        .claims(&client.id_token_verifier(), &Nonce::new(pending.nonce))
        .map_err(ApiError::internal)?;

    let display_name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.as_str().to_owned());
    let account = identity::find_or_create_oidc_user(
        &state.pool,
        OidcProfile {
            provider: provider_id,
            subject: claims.subject().as_str(),
            email: claims.email().map(|e| e.as_str()),
            email_verified: claims.email_verified().unwrap_or(false),
            display_name: display_name.as_deref(),
        },
    )
    .await?;
    identity::touch_last_login(&state.pool, account.user_id).await?;
    audit_auth(state, account, "auth.login.oidc").await?;

    let jar = jar.remove(Cookie::build(STATE_COOKIE).path("/api/auth/oidc").build());
    let (jar, _token) = start_session(state, jar, account, pending.remember, headers).await?;
    Ok(jar)
}

fn redis_key(state: &str) -> String {
    format!("oidc:pending:{state}")
}
