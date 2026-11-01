//! Who may open a sheet's share link, and what a link viewer may ask of its visuals
//! (docs/adr/0005). Used by the REST view of a link and by `shared_batch` on the data socket.

use chrono::{DateTime, Utc};
use lattice_planner::FilterOp;
use lattice_security::Principal;
use lattice_sheets::{self as sheets, LinkAudience, SharedSheet, Sheet};
use lattice_visuals::SheetContext;

use crate::error::ApiError;
use crate::state::AppState;

/// Tokens are 43 characters; anything much longer isn't worth a database lookup.
const MAX_TOKEN_LEN: usize = 100;

/// Opens the sheet behind `token` for `viewer` (None when not signed in).
///
/// A missing, switched-off or expired link is "not found" whoever asks, so a link's existence
/// can't be probed. Links to invited people or to the workspace need a signed-in viewer who
/// qualifies; project members always qualify.
pub async fn open_link(
    state: &AppState,
    token: &str,
    viewer: Option<&Principal>,
) -> Result<SharedSheet, ApiError> {
    if token.is_empty() || token.len() > MAX_TOKEN_LEN {
        return Err(ApiError::NotFound);
    }
    let Some((shared, audience)) = sheets::by_token(&state.pool, token)
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::NotFound);
    };
    if audience == LinkAudience::Off || shared.expires_at.is_some_and(|at| at <= Utc::now()) {
        return Err(ApiError::NotFound);
    }
    if audience == LinkAudience::Public {
        return Ok(shared);
    }
    let viewer = viewer.ok_or(ApiError::Unauthenticated)?;
    let project_member = lattice_project::role_of(
        &state.pool,
        viewer.workspace_id,
        shared.sheet.project_id,
        viewer.user_id,
    )
    .await?
    .is_some();
    let allowed = project_member
        || match audience {
            LinkAudience::Workspace => viewer.workspace_id == shared.workspace_id,
            LinkAudience::Invited => {
                sheets::is_member(&state.pool, shared.sheet.id, viewer.user_id)
                    .await
                    .map_err(ApiError::internal)?
            }
            LinkAudience::Off | LinkAudience::Public => false,
        };
    if allowed {
        Ok(shared)
    } else {
        Err(ApiError::Forbidden)
    }
}

/// Narrows a link viewer's interactive state to what the sheet itself offers: values for the
/// sheet's own filters, and selections on its visuals' dimensions. Anything else is refused,
/// so a link can't be turned into a general query interface.
pub fn shared_context(sheet: &Sheet, ctx: SheetContext) -> Result<SheetContext, String> {
    for f in &ctx.filters {
        let defined = sheet.filters.iter().any(|d| d.column == f.column);
        if !defined || !matches!(f.op, FilterOp::Between | FilterOp::In | FilterOp::Eq) {
            return Err(format!("{:?} isn't a filter of this sheet", f.column));
        }
    }
    for s in &ctx.selections {
        let dimension = sheet.visuals.iter().any(|v| {
            v.id == s.visual_id
                && v.spec
                    .x
                    .iter()
                    .chain(v.spec.series.iter())
                    .chain(v.spec.drill.iter())
                    .any(|d| d.column == s.filter.column)
        });
        // Eq/In pick categories; Between picks a time bucket or a histogram bin.
        let op = matches!(s.filter.op, FilterOp::Eq | FilterOp::In | FilterOp::Between);
        if !dimension || !op {
            return Err(format!("can't select on {:?} here", s.filter.column));
        }
    }
    Ok(ctx)
}

/// When the newest source behind the sheet's visuals last synced.
pub async fn data_refreshed_at(
    state: &AppState,
    sheet: &Sheet,
) -> Result<Option<DateTime<Utc>>, ApiError> {
    let tables: Vec<uuid::Uuid> = sheet.visuals.iter().map(|v| v.spec.table_id).collect();
    Ok(sqlx::query_scalar!(
        "SELECT max(s.synced_at) FROM sources s
         JOIN source_tables t ON t.source_id = s.id
         WHERE t.id = ANY($1) AND s.project_id = $2",
        &tables,
        sheet.project_id
    )
    .fetch_one(&state.pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_planner::{Dimension, Filter, FilterValue, Scalar};
    use lattice_sheets::{Layout, SheetFilterDef, SheetFilterKind, Visual};
    use lattice_visuals::{Selection, SelectionScope, VisualKind, VisualOptions, VisualSpec};
    use uuid::Uuid;

    fn sheet() -> Sheet {
        let dim = |c: &str| Dimension {
            column: c.into(),
            bucket: None,
            bin: None,
        };
        Sheet {
            id: Uuid::nil(),
            project_id: Uuid::nil(),
            name: "Revenue Review".into(),
            position: 1,
            filters: vec![SheetFilterDef {
                id: Uuid::nil(),
                column: "region".into(),
                label: "Region".into(),
                kind: SheetFilterKind::Values,
                value: None,
                options: vec![],
                preset: None,
            }],
            visuals: vec![Visual {
                id: Uuid::from_u128(1),
                spec: VisualSpec {
                    kind: VisualKind::Bar,
                    title: "By category".into(),
                    table_id: Uuid::nil(),
                    x: Some(dim("category")),
                    series: None,
                    measures: vec![],
                    columns: vec![],
                    drill: vec![dim("store_id")],
                    filters: vec![],
                    sort: None,
                    limit: None,
                    options: VisualOptions::default(),
                },
                layout: Layout {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 4,
                },
            }],
            version: 1,
            updated_at: Utc::now(),
            link_audience: LinkAudience::Public,
        }
    }

    fn eq(column: &str, value: &str) -> Filter {
        Filter {
            column: column.into(),
            op: FilterOp::Eq,
            value: Some(FilterValue::One(Scalar::Text(value.into()))),
        }
    }

    fn select(visual: u128, filter: Filter) -> Selection {
        Selection {
            visual_id: Uuid::from_u128(visual),
            table_id: Uuid::nil(),
            scope: SelectionScope::Table,
            filter,
        }
    }

    #[test]
    fn link_viewers_use_only_the_sheets_filters_and_dimensions() {
        let ok = SheetContext {
            filters: vec![eq("region", "EMEA")],
            selections: vec![
                select(1, eq("category", "Outdoor")),
                select(1, eq("store_id", "7")),
            ],
        };
        assert!(shared_context(&sheet(), ok).is_ok());

        for bad in [
            SheetContext {
                filters: vec![eq("customer_id", "42")],
                ..Default::default()
            },
            SheetContext {
                selections: vec![select(1, eq("amount", "10"))],
                ..Default::default()
            },
            SheetContext {
                selections: vec![select(2, eq("category", "Outdoor"))],
                ..Default::default()
            },
            SheetContext {
                filters: vec![Filter {
                    op: FilterOp::Contains,
                    ..eq("region", "E")
                }],
                ..Default::default()
            },
        ] {
            assert!(shared_context(&sheet(), bad.clone()).is_err(), "{bad:?}");
        }
    }
}
