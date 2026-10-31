use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum_extra::extract::cookie::CookieJar;
use lattice_security::audit::{self, Actor, AuditEvent};
use lattice_security::identity::{self, Account, NewAccount, PasswordCredentials};
use lattice_security::password::{burn_verification_time, hash_password, verify_password};

use super::{SESSION_COOKIE, clear_session_cookie, start_session};
use crate::dto::{LoginRequest, MeResponse, SignupRequest};
use crate::error::ApiError;
use crate::state::AppState;

const MIN_PASSWORD_CHARS: usize = 10;
/// Argon2 cost is per byte; cap input so a giant password can't be used to burn CPU.
const MAX_PASSWORD_BYTES: usize = 512;

/// "Create a workspace": a new user who administers a new workspace.
pub async fn signup(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(req): Json<SignupRequest>,
) -> Result<(StatusCode, CookieJar, Json<MeResponse>), ApiError> {
    let email = normalize_email(&req.email)?;
    let display_name = required_text(&req.display_name, "Your name", 80)?;
    let workspace_name = required_text(&req.workspace_name, "Workspace name", 80)?;
    validate_password(&req.password)?;

    let password = req.password;
    let hash = tokio::task::spawn_blocking(move || hash_password(&password)).await??;
    let account = identity::create_account_with_workspace(
        &state.pool,
        NewAccount {
            email: &email,
            display_name,
            password_hash: Some(&hash),
            workspace_name,
        },
    )
    .await?;
    audit_auth(&state, account, "auth.signup").await?;

    let (jar, token) = start_session(&state, jar, account, req.remember, &headers).await?;
    let me = load_me(&state, &token).await?;
    Ok((StatusCode::CREATED, jar, Json(me)))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<(CookieJar, Json<MeResponse>), ApiError> {
    // Malformed input gets the same answer as a wrong password: no account-existence oracle.
    let email = normalize_email(&req.email).map_err(|_| ApiError::InvalidCredentials)?;
    if req.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::InvalidCredentials);
    }
    let credentials = identity::find_password_credentials(&state.pool, &email).await?;

    let password = req.password;
    let user_id = tokio::task::spawn_blocking(move || match credentials {
        Some(PasswordCredentials {
            user_id,
            password_hash: Some(hash),
        }) => verify_password(&password, &hash).then_some(user_id),
        _ => {
            burn_verification_time(&password);
            None
        }
    })
    .await?;
    let Some(user_id) = user_id else {
        tracing::info!(
            email_domain = email.split('@').nth(1),
            "failed password login"
        );
        return Err(ApiError::InvalidCredentials);
    };

    let workspace_id = identity::primary_workspace(&state.pool, user_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    let account = Account {
        user_id,
        workspace_id,
    };
    identity::touch_last_login(&state.pool, user_id).await?;
    audit_auth(&state, account, "auth.login.password").await?;

    let (jar, token) = start_session(&state, jar, account, req.remember, &headers).await?;
    let me = load_me(&state, &token).await?;
    Ok((jar, Json(me)))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(StatusCode, CookieJar), ApiError> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state.sessions.revoke(cookie.value()).await?;
    }
    Ok((StatusCode::NO_CONTENT, clear_session_cookie(jar)))
}

pub(crate) async fn load_me(state: &AppState, token: &str) -> Result<MeResponse, ApiError> {
    state
        .sessions
        .lookup(token)
        .await?
        .map(MeResponse::from)
        .ok_or_else(|| ApiError::internal("session vanished immediately after creation"))
}

pub(crate) async fn audit_auth(
    state: &AppState,
    account: Account,
    action: &str,
) -> Result<(), ApiError> {
    audit::record(
        &state.pool,
        AuditEvent {
            workspace_id: Some(account.workspace_id),
            actor: Actor::User(account.user_id),
            action,
            target: None,
            detail: serde_json::json!({}),
        },
    )
    .await?;
    Ok(())
}

pub(crate) fn normalize_email(raw: &str) -> Result<String, ApiError> {
    let email = raw.trim().to_lowercase();
    let valid = email.len() <= 254
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
        && !email.chars().any(char::is_whitespace);
    if valid {
        Ok(email)
    } else {
        Err(ApiError::BadRequest("Enter a valid email address.".into()))
    }
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Use at least {MIN_PASSWORD_CHARS} characters for your password."
        )));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::BadRequest("That password is too long.".into()));
    }
    Ok(())
}

pub(crate) fn required_text<'a>(
    raw: &'a str,
    field: &str,
    max_chars: usize,
) -> Result<&'a str, ApiError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ApiError::BadRequest(format!("{field} is required.")));
    }
    if value.chars().count() > max_chars {
        return Err(ApiError::BadRequest(format!(
            "{field} must be at most {max_chars} characters."
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert_eq!(normalize_email("  Ana@Acme.COM ").unwrap(), "ana@acme.com");
        for bad in [
            "",
            "ana",
            "@acme.com",
            "ana@acme",
            "ana@.com",
            "a na@acme.com",
            "ana@acme.",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn passwords() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("long-enough-1").is_ok());
        assert!(validate_password(&"x".repeat(513)).is_err());
    }

    #[test]
    fn text_fields_are_trimmed_and_bounded() {
        assert_eq!(required_text("  Acme  ", "Name", 80).unwrap(), "Acme");
        assert!(required_text("   ", "Name", 80).is_err());
        assert!(required_text(&"é".repeat(81), "Name", 80).is_err());
    }
}
