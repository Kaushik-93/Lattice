use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lattice_project::ProjectError;
use lattice_security::IdentityError;

use crate::dto::{ApiErrorBody, ApiErrorDetail};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Every handler error. The JSON body is always `{"error": {"code", "message"}}`;
/// internal details are logged, never returned.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("sign in required")]
    Unauthenticated,
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("you don't have access to this")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("cross-origin request rejected")]
    BadOrigin,
    #[error("internal error")]
    Internal(#[source] BoxError),
}

impl ApiError {
    pub fn internal(err: impl Into<BoxError>) -> Self {
        Self::Internal(err.into())
    }

    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            ApiError::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            ApiError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ApiError::BadOrigin => (StatusCode::FORBIDDEN, "bad_origin"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::Internal(err) = &self {
            tracing::error!(error = %err, source = ?err.source(), "request failed");
        }
        let (status, code) = self.status_and_code();
        let body = ApiErrorBody {
            error: ApiErrorDetail {
                code: code.to_owned(),
                message: self.to_string(),
            },
        };
        (status, Json(body)).into_response()
    }
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        match err {
            IdentityError::EmailTaken => {
                ApiError::Conflict("an account with this email already exists".into())
            }
            other => ApiError::internal(other),
        }
    }
}

macro_rules! internal_from {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for ApiError {
            fn from(err: $ty) -> Self {
                ApiError::internal(err)
            }
        }
    )*};
}

internal_from!(
    sqlx::Error,
    ProjectError,
    redis::RedisError,
    tokio::task::JoinError,
    serde_json::Error,
    lattice_security::password::PasswordError,
);
