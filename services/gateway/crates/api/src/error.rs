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
    /// The request was well-formed but its content can't be used (a bad file, a refused connection).
    #[error("{0}")]
    Unprocessable(String),
    #[error("that file is larger than the {0} MB limit")]
    PayloadTooLarge(u64),
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
            ApiError::Unprocessable(_) => (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable"),
            ApiError::PayloadTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
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

impl From<lattice_engine::EngineError> for ApiError {
    fn from(err: lattice_engine::EngineError) -> Self {
        use lattice_engine::EngineError as E;
        match err {
            E::Query(m) | E::Spreadsheet(m) => ApiError::Unprocessable(m),
            E::Timeout(d) => {
                ApiError::Unprocessable(format!("That took longer than {}s.", d.as_secs()))
            }
            E::Busy => {
                ApiError::Unprocessable("The project is busy; try again in a moment.".into())
            }
            other => ApiError::internal(other),
        }
    }
}

impl From<lattice_planner::PlanError> for ApiError {
    fn from(err: lattice_planner::PlanError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<lattice_connectors::ConnectorError> for ApiError {
    fn from(err: lattice_connectors::ConnectorError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<lattice_sheets::SheetError> for ApiError {
    fn from(err: lattice_sheets::SheetError) -> Self {
        use lattice_sheets::SheetError as E;
        match err {
            E::Conflict { .. } => ApiError::Conflict(
                "Someone else saved this sheet since you opened it. Reload to see their changes."
                    .into(),
            ),
            E::Invalid(m) => ApiError::BadRequest(m),
            E::TooMany => ApiError::Unprocessable(err.to_string()),
            other => ApiError::internal(other),
        }
    }
}

impl From<axum::extract::multipart::MultipartError> for ApiError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        ApiError::BadRequest(format!("upload failed: {}", err.body_text()))
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
    lattice_catalog::CatalogError,
    lattice_security::CryptoError,
    std::io::Error,
    object_store::Error,
);
