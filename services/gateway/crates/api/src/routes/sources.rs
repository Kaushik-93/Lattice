use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use lattice_catalog::{self as catalog, SourceDetail};
use lattice_connectors::{MAX_UPLOAD_BYTES, UploadKind};
use lattice_security::ProjectRole;
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;

use crate::auth::Authed;
use crate::dto::{CreateDatabaseSourceRequest, SourceListResponse};
use crate::error::ApiError;
use crate::routes::projects::require_role;
use crate::sources::{self, safe_file_name};
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
) -> Result<Json<SourceListResponse>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Viewer).await?;
    Ok(Json(SourceListResponse {
        sources: catalog::list_sources(&state.pool, project_id).await?,
    }))
}

pub async fn get(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<SourceDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Viewer).await?;
    catalog::source_detail(&state.pool, project_id, source_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn create_database(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateDatabaseSourceRequest>,
) -> Result<(StatusCode, Json<SourceDetail>), ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    let detail = sources::create_database_source(&state, &me, project_id, req).await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

/// `multipart/form-data` with one `file` field. Streams to disk; never buffers the file in memory.
pub async fn upload(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path(project_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<SourceDetail>), ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    let mut field = loop {
        match multipart.next_field().await? {
            Some(field) if field.name() == Some("file") => break field,
            Some(_) => continue,
            None => {
                return Err(ApiError::BadRequest(
                    "Attach the file in a field named \"file\".".into(),
                ));
            }
        }
    };
    let file_name = safe_file_name(field.file_name().unwrap_or_default());
    if UploadKind::from_file_name(&file_name).is_none() {
        return Err(ApiError::BadRequest(
            "Lattice reads CSV, TSV, JSON, NDJSON, Parquet, Excel (.xlsx) and SQLite files.".into(),
        ));
    }

    let incoming_dir = state
        .engine
        .config()
        .project_dir(project_id)
        .join("uploads")
        .join("incoming");
    tokio::fs::create_dir_all(&incoming_dir).await?;
    let incoming = incoming_dir.join(Uuid::new_v4().simple().to_string());
    let mut file = tokio::fs::File::create(&incoming).await?;
    let mut size = 0u64;
    let streamed = async {
        while let Some(chunk) = field.chunk().await? {
            size += chunk.len() as u64;
            if size > MAX_UPLOAD_BYTES {
                return Err(ApiError::PayloadTooLarge(MAX_UPLOAD_BYTES / (1024 * 1024)));
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(err) = streamed {
        let _ = tokio::fs::remove_file(&incoming).await;
        return Err(err);
    }
    if size == 0 {
        let _ = tokio::fs::remove_file(&incoming).await;
        return Err(ApiError::BadRequest("That file is empty.".into()));
    }

    let detail =
        sources::create_file_source(&state, &me, project_id, incoming, &file_name, size).await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

pub async fn refresh(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<SourceDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    Ok(Json(
        sources::refresh(&state, &me, project_id, source_id).await?,
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Admin).await?;
    sources::delete(&state, &me, project_id, source_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn profile(
    State(state): State<AppState>,
    Authed(me): Authed,
    Path((project_id, table_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<SourceDetail>, ApiError> {
    require_role(&state, &me, project_id, ProjectRole::Editor).await?;
    Ok(Json(
        sources::profile_table(&state, project_id, table_id).await?,
    ))
}
