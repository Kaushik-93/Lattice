//! Users, workspaces, sessions and identity-provider links.

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::rbac::{UnknownRole, WorkspaceRole};
use crate::token::{SESSION_TOKEN_LEN, SessionToken, hash_token};

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("an account with this email already exists")]
    EmailTaken,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("corrupt role in database: {0}")]
    CorruptRole(#[from] UnknownRole),
}

/// The authenticated caller of a request, resolved from the session cookie.
#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    pub workspace_role: WorkspaceRole,
    pub session_expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SessionStore {
    pool: PgPool,
}

impl SessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        ttl: TimeDelta,
        user_agent: Option<&str>,
    ) -> Result<(SessionToken, DateTime<Utc>), IdentityError> {
        let token = SessionToken::generate();
        let expires_at = Utc::now() + ttl;
        sqlx::query!(
            "INSERT INTO sessions (token_hash, user_id, workspace_id, expires_at, user_agent)
             VALUES ($1, $2, $3, $4, $5)",
            &token.hash()[..],
            user_id,
            workspace_id,
            expires_at,
            user_agent.map(|ua| truncate(ua, 256)),
        )
        .execute(&self.pool)
        .await?;
        Ok((token, expires_at))
    }

    /// One indexed join: this runs on every authenticated request.
    pub async fn lookup(&self, token: &str) -> Result<Option<Principal>, IdentityError> {
        if token.len() != SESSION_TOKEN_LEN {
            return Ok(None);
        }
        let row = sqlx::query!(
            r#"SELECT s.user_id, s.workspace_id, s.expires_at, u.email, u.display_name,
                      w.name AS workspace_name, w.slug AS workspace_slug, wm.role AS workspace_role
               FROM sessions s
               JOIN users u ON u.id = s.user_id
               JOIN workspaces w ON w.id = s.workspace_id
               JOIN workspace_members wm ON wm.workspace_id = s.workspace_id AND wm.user_id = s.user_id
               WHERE s.token_hash = $1 AND s.expires_at > now()"#,
            &hash_token(token)[..],
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| {
            Ok(Principal {
                user_id: r.user_id,
                email: r.email,
                display_name: r.display_name,
                workspace_id: r.workspace_id,
                workspace_name: r.workspace_name,
                workspace_slug: r.workspace_slug,
                workspace_role: r.workspace_role.parse()?,
                session_expires_at: r.expires_at,
            })
        })
        .transpose()
    }

    pub async fn revoke(&self, token: &str) -> Result<(), IdentityError> {
        sqlx::query!(
            "DELETE FROM sessions WHERE token_hash = $1",
            &hash_token(token)[..]
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn purge_expired(&self) -> Result<u64, IdentityError> {
        let done = sqlx::query!("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }
}

pub struct NewAccount<'a> {
    pub email: &'a str,
    pub display_name: &'a str,
    pub password_hash: Option<&'a str>,
    pub workspace_name: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct Account {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
}

/// Creates a user together with a new workspace they administer.
pub async fn create_account_with_workspace(
    pool: &PgPool,
    new: NewAccount<'_>,
) -> Result<Account, IdentityError> {
    let mut tx = pool.begin().await?;
    let account = create_account_in(&mut tx, &new).await?;
    tx.commit().await?;
    Ok(account)
}

async fn create_account_in(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewAccount<'_>,
) -> Result<Account, IdentityError> {
    let user_id = sqlx::query_scalar!(
        "INSERT INTO users (email, display_name, password_hash) VALUES ($1, $2, $3)
         ON CONFLICT (email) DO NOTHING RETURNING id",
        new.email,
        new.display_name,
        new.password_hash,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(IdentityError::EmailTaken)?;

    let workspace_id = insert_workspace(tx, new.workspace_name).await?;
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'admin')",
        workspace_id,
        user_id,
    )
    .execute(&mut **tx)
    .await?;

    Ok(Account {
        user_id,
        workspace_id,
    })
}

/// Inserts a workspace with a unique slug derived from its name ("Acme Retail" → "acme-retail",
/// then "acme-retail-2", ... on collision).
pub async fn insert_workspace(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
) -> Result<Uuid, IdentityError> {
    let base = slugify(name);
    for attempt in 1..=50u32 {
        let slug = if attempt == 1 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let id = sqlx::query_scalar!(
            "INSERT INTO workspaces (name, slug) VALUES ($1, $2) ON CONFLICT (slug) DO NOTHING RETURNING id",
            name,
            slug,
        )
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(id) = id {
            return Ok(id);
        }
    }
    // 50 collisions on one base slug: fall back to a random suffix, which cannot realistically collide.
    let slug = format!("{base}-{}", &Uuid::new_v4().simple().to_string()[..8]);
    Ok(sqlx::query_scalar!(
        "INSERT INTO workspaces (name, slug) VALUES ($1, $2) RETURNING id",
        name,
        slug,
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Adds a user with no password (they can only sign in through an identity provider) to
/// an existing workspace. Used for invitations and demo data.
pub async fn add_member_without_login(
    pool: &PgPool,
    workspace_id: Uuid,
    email: &str,
    display_name: &str,
    role: WorkspaceRole,
) -> Result<Uuid, IdentityError> {
    let mut tx = pool.begin().await?;
    let user_id = sqlx::query_scalar!(
        "INSERT INTO users (email, display_name) VALUES ($1, $2)
         ON CONFLICT (email) DO NOTHING RETURNING id",
        email,
        display_name,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(IdentityError::EmailTaken)?;
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
        workspace_id,
        user_id,
        role.as_str(),
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(user_id)
}

pub struct PasswordCredentials {
    pub user_id: Uuid,
    pub password_hash: Option<String>,
}

pub async fn find_password_credentials(
    pool: &PgPool,
    email: &str,
) -> Result<Option<PasswordCredentials>, IdentityError> {
    Ok(sqlx::query_as!(
        PasswordCredentials,
        "SELECT id AS user_id, password_hash FROM users WHERE email = $1",
        email,
    )
    .fetch_optional(pool)
    .await?)
}

/// The workspace a user lands in after signing in: the first one they joined.
pub async fn primary_workspace(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<Uuid>, IdentityError> {
    Ok(sqlx::query_scalar!(
        "SELECT workspace_id FROM workspace_members WHERE user_id = $1 ORDER BY created_at LIMIT 1",
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn touch_last_login(pool: &PgPool, user_id: Uuid) -> Result<(), IdentityError> {
    sqlx::query!(
        "UPDATE users SET last_login_at = now() WHERE id = $1",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Claims taken from a verified OIDC ID token.
pub struct OidcProfile<'a> {
    pub provider: &'a str,
    pub subject: &'a str,
    pub email: Option<&'a str>,
    pub email_verified: bool,
    pub display_name: Option<&'a str>,
}

/// Resolves an identity-provider login to an account:
/// 1. a known (provider, subject) pair signs in as its linked user;
/// 2. otherwise a *verified* email that matches an existing user links to that user;
/// 3. otherwise a new user and workspace are created.
pub async fn find_or_create_oidc_user(
    pool: &PgPool,
    profile: OidcProfile<'_>,
) -> Result<Account, IdentityError> {
    let mut tx = pool.begin().await?;

    let linked = sqlx::query_scalar!(
        "SELECT user_id FROM user_identities WHERE provider = $1 AND subject = $2",
        profile.provider,
        profile.subject,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let email = profile.email.map(str::to_lowercase);
    let user_id = match linked {
        Some(id) => Some(id),
        None => match (&email, profile.email_verified) {
            (Some(email), true) => {
                sqlx::query_scalar!("SELECT id FROM users WHERE email = $1", email)
                    .fetch_optional(&mut *tx)
                    .await?
            }
            _ => None,
        },
    };

    let account = match user_id {
        Some(user_id) => {
            let workspace_id = sqlx::query_scalar!(
                "SELECT workspace_id FROM workspace_members WHERE user_id = $1 ORDER BY created_at LIMIT 1",
                user_id,
            )
            .fetch_optional(&mut *tx)
            .await?;
            let workspace_id = match workspace_id {
                Some(id) => id,
                None => {
                    let id = insert_workspace(&mut tx, "My workspace").await?;
                    sqlx::query!(
                        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'admin')",
                        id,
                        user_id,
                    )
                    .execute(&mut *tx)
                    .await?;
                    id
                }
            };
            Account {
                user_id,
                workspace_id,
            }
        }
        None => {
            let email = email.clone().unwrap_or_else(|| {
                format!("{}@{}.oidc.invalid", profile.subject, profile.provider).to_lowercase()
            });
            let display_name = profile
                .display_name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| email.split('@').next().unwrap_or("New user"));
            let display_name = truncate(display_name.trim(), 80);
            let workspace_name = format!("{}'s workspace", truncate(display_name, 60));
            create_account_in(
                &mut tx,
                &NewAccount {
                    email: &email,
                    display_name,
                    password_hash: None,
                    workspace_name: &workspace_name,
                },
            )
            .await?
        }
    };

    sqlx::query!(
        "INSERT INTO user_identities (provider, subject, user_id, email) VALUES ($1, $2, $3, $4)
         ON CONFLICT (provider, subject) DO UPDATE SET email = EXCLUDED.email",
        profile.provider,
        profile.subject,
        account.user_id,
        email,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(account)
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "workspace".to_owned()
    } else {
        slug.to_owned()
    }
}

fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Acme Retail"), "acme-retail");
        assert_eq!(slugify("  People & Headcount!! "), "people-headcount");
        assert_eq!(slugify("Ünïcödé"), "n-c-d");
        assert_eq!(slugify("***"), "workspace");
        assert!(slugify(&"x".repeat(200)).len() <= 40);
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("héllo", 2), "hé");
        assert_eq!(truncate("hi", 10), "hi");
    }
}
