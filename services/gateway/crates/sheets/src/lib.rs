//! Sheets and their visuals: storage, ordering, optimistic-concurrency saves and link sharing
//! (docs/adr/0005).
//!
//! A save replaces the whole sheet — name, filters, visuals and layout — in one transaction,
//! guarded by the version the editor started from. That keeps autosave and undo/redo simple
//! (any client state can be saved) while never silently overwriting someone else's edit.

use chrono::{DateTime, Utc};
use lattice_planner::{FilterValue, Scalar};
use lattice_visuals::VisualSpec;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use ts_rs::TS;
use uuid::Uuid;

pub const MAX_SHEETS_PER_PROJECT: i64 = 100;
pub const MAX_VISUALS_PER_SHEET: usize = 40;
pub const MAX_FILTERS_PER_SHEET: usize = 8;
/// Columns of the sheet grid.
pub const GRID_COLUMNS: u16 = 12;

#[derive(Debug, thiserror::Error)]
pub enum SheetError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("corrupt sheet row: {0}")]
    Corrupt(String),
    #[error("someone else saved this sheet first")]
    Conflict { current: i64 },
    #[error("{0}")]
    Invalid(String),
    #[error("too many sheets in this project (the limit is {MAX_SHEETS_PER_PROJECT})")]
    TooMany,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LinkAudience {
    /// No link.
    Off,
    /// Signed-in people added to the sheet by name.
    Invited,
    /// Anyone signed in to the sheet's workspace.
    Workspace,
    /// Anyone with the link, no sign-in.
    Public,
}

impl LinkAudience {
    const fn as_str(self) -> &'static str {
        match self {
            LinkAudience::Off => "off",
            LinkAudience::Invited => "invited",
            LinkAudience::Workspace => "workspace",
            LinkAudience::Public => "public",
        }
    }

    fn parse(s: &str) -> Result<Self, SheetError> {
        match s {
            "off" => Ok(Self::Off),
            "invited" => Ok(Self::Invited),
            "workspace" => Ok(Self::Workspace),
            "public" => Ok(Self::Public),
            other => Err(SheetError::Corrupt(format!("link audience {other:?}"))),
        }
    }
}

/// A sheet tab.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SheetSummary {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
    #[ts(type = "number")]
    pub visual_count: i64,
    pub link_audience: LinkAudience,
    pub updated_at: DateTime<Utc>,
}

/// A filter shown in the sheet header (e.g. a date range or a region picker). Its value
/// applies to every visual whose table has the column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SheetFilterDef {
    pub id: Uuid,
    pub column: String,
    pub label: String,
    pub kind: SheetFilterKind,
    /// The default value: `[from, to]` for a date range, the picked values otherwise. None
    /// means unfiltered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub value: Option<FilterValue>,
    /// The values a picker offers, saved with the sheet so link viewers (who can't run
    /// queries of their own) can choose among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Scalar>>")]
    pub options: Vec<Scalar>,
    /// A date range relative to today, worked out afresh whenever the sheet opens (on the
    /// fiscal calendar); `value` then holds what it last resolved to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preset: Option<DatePreset>,
}

/// Date ranges to date (docs/adr/0006 decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatePreset {
    FiscalYearToDate,
    FiscalQuarterToDate,
    MonthToDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SheetFilterKind {
    DateRange,
    Values,
}

/// Placement on the 12-column sheet grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Layout {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Visual {
    pub id: Uuid,
    pub spec: VisualSpec,
    pub layout: Layout,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Sheet {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub position: i32,
    pub filters: Vec<SheetFilterDef>,
    pub visuals: Vec<Visual>,
    #[ts(type = "number")]
    pub version: i64,
    pub updated_at: DateTime<Utc>,
    pub link_audience: LinkAudience,
}

/// The whole editable state of a sheet, as saved by the editor.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SaveSheet {
    /// The version this edit started from.
    #[ts(type = "number")]
    pub version: i64,
    pub name: String,
    pub filters: Vec<SheetFilterDef>,
    pub visuals: Vec<Visual>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SheetSharing {
    pub audience: LinkAudience,
    /// The link's secret part; present while a link exists.
    pub token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub allow_export: bool,
    pub track_views: bool,
    pub allow_copilot: bool,
    pub members: Vec<SheetMember>,
    #[ts(type = "number")]
    pub view_count: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SheetMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SharingUpdate {
    pub audience: LinkAudience,
    /// Days until the link stops working; None for never.
    #[serde(default)]
    pub expires_in_days: Option<u16>,
    pub allow_export: bool,
    pub track_views: bool,
    #[serde(default)]
    pub allow_copilot: bool,
    /// Issue a new link, so the old one stops working.
    #[serde(default)]
    pub rotate: bool,
}

/// A sheet opened through its link.
#[derive(Debug, Clone)]
pub struct SharedSheet {
    pub sheet: Sheet,
    pub workspace_id: Uuid,
    pub project_name: String,
    pub shared_by: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub allow_export: bool,
    pub track_views: bool,
}

/// Checks a save before it touches the database.
pub fn validate(save: &SaveSheet) -> Result<(), SheetError> {
    let invalid = |m: String| Err(SheetError::Invalid(m));
    let name = save.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return invalid("A sheet name has 1–80 characters.".into());
    }
    if save.visuals.len() > MAX_VISUALS_PER_SHEET {
        return invalid(format!(
            "A sheet holds at most {MAX_VISUALS_PER_SHEET} visuals."
        ));
    }
    if save.filters.len() > MAX_FILTERS_PER_SHEET {
        return invalid(format!(
            "A sheet has at most {MAX_FILTERS_PER_SHEET} filters."
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for v in &save.visuals {
        if !ids.insert(v.id) {
            return invalid("Each visual needs its own id.".into());
        }
        let l = v.layout;
        if l.w == 0 || l.h == 0 || l.x + l.w > GRID_COLUMNS || l.h > 40 || l.y > 1000 {
            return invalid(format!("Visual {} is placed outside the grid.", v.id));
        }
        if v.spec.title.chars().count() > 120 {
            return invalid("A visual title has at most 120 characters.".into());
        }
    }
    for f in &save.filters {
        if f.label.trim().is_empty() || f.label.chars().count() > 60 {
            return invalid("A filter label has 1–60 characters.".into());
        }
        if f.options.len() > 200 {
            return invalid("A filter offers at most 200 values.".into());
        }
        if f.preset.is_some() && f.kind != SheetFilterKind::DateRange {
            return invalid(format!(
                "Filter {:?}: only date ranges have presets.",
                f.label
            ));
        }
        let ok = match (&f.kind, &f.value) {
            (_, None) => true,
            (SheetFilterKind::DateRange, Some(FilterValue::Many(v))) => {
                v.len() == 2 && v.iter().all(|s| matches!(s, Scalar::Text(_)))
            }
            (SheetFilterKind::Values, Some(FilterValue::Many(v))) => (1..=200).contains(&v.len()),
            _ => false,
        };
        if !ok {
            return invalid(format!(
                "The value of filter {:?} doesn't fit its kind.",
                f.label
            ));
        }
    }
    Ok(())
}

pub async fn list(pool: &PgPool, project_id: Uuid) -> Result<Vec<SheetSummary>, SheetError> {
    let rows = sqlx::query!(
        r#"SELECT s.id, s.name, s.position, s.link_audience, s.updated_at,
                  (SELECT count(*) FROM visuals v WHERE v.sheet_id = s.id) AS "visual_count!"
           FROM sheets s WHERE s.project_id = $1 ORDER BY s.position, s.created_at"#,
        project_id
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(SheetSummary {
                id: r.id,
                name: r.name,
                position: r.position,
                visual_count: r.visual_count,
                link_audience: LinkAudience::parse(&r.link_audience)?,
                updated_at: r.updated_at,
            })
        })
        .collect()
}

pub async fn create(
    pool: &PgPool,
    project_id: Uuid,
    name: &str,
    filters: &[SheetFilterDef],
    user_id: Uuid,
) -> Result<Uuid, SheetError> {
    validate(&SaveSheet {
        version: 0,
        name: name.to_owned(),
        filters: filters.to_vec(),
        visuals: vec![],
    })?;
    let mut tx = pool.begin().await?;
    // Serialise creations per project so positions and the limit stay exact.
    sqlx::query!(
        "SELECT id FROM projects WHERE id = $1 FOR UPDATE",
        project_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let stats = sqlx::query!(
        r#"SELECT count(*) AS "count!", COALESCE(max(position), 0) AS "last!"
           FROM sheets WHERE project_id = $1"#,
        project_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if stats.count >= MAX_SHEETS_PER_PROJECT {
        return Err(SheetError::TooMany);
    }
    let id = sqlx::query_scalar!(
        "INSERT INTO sheets (project_id, name, position, filters, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $5) RETURNING id",
        project_id,
        name.trim(),
        stats.last + 1,
        to_json(filters)?,
        user_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn get(
    pool: &PgPool,
    project_id: Uuid,
    sheet_id: Uuid,
) -> Result<Option<Sheet>, SheetError> {
    let Some(row) = sqlx::query!(
        "SELECT id, project_id, name, position, filters, version, updated_at, link_audience
         FROM sheets WHERE id = $1 AND project_id = $2",
        sheet_id,
        project_id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(Sheet {
        id: row.id,
        project_id: row.project_id,
        name: row.name,
        position: row.position,
        filters: from_json(row.filters, "filters")?,
        visuals: visuals(pool, row.id).await?,
        version: row.version,
        updated_at: row.updated_at,
        link_audience: LinkAudience::parse(&row.link_audience)?,
    }))
}

async fn visuals(pool: &PgPool, sheet_id: Uuid) -> Result<Vec<Visual>, SheetError> {
    let rows = sqlx::query!(
        "SELECT id, spec, layout FROM visuals WHERE sheet_id = $1 ORDER BY position",
        sheet_id
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Visual {
                id: r.id,
                spec: from_json(r.spec, "visual spec")?,
                layout: from_json(r.layout, "visual layout")?,
            })
        })
        .collect()
}

/// Replaces the sheet's editable state if nobody saved since `save.version`; returns the new
/// version.
pub async fn save(
    pool: &PgPool,
    project_id: Uuid,
    sheet_id: Uuid,
    save: &SaveSheet,
    user_id: Uuid,
) -> Result<Option<i64>, SheetError> {
    validate(save)?;
    let mut tx = pool.begin().await?;
    let Some(current) = sqlx::query_scalar!(
        "SELECT version FROM sheets WHERE id = $1 AND project_id = $2 FOR UPDATE",
        sheet_id,
        project_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    if current != save.version {
        return Err(SheetError::Conflict { current });
    }
    let version = sqlx::query_scalar!(
        "UPDATE sheets SET name = $2, filters = $3, version = version + 1, updated_by = $4,
                updated_at = now()
         WHERE id = $1 RETURNING version",
        sheet_id,
        save.name.trim(),
        to_json(&save.filters)?,
        user_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    let ids: Vec<Uuid> = save.visuals.iter().map(|v| v.id).collect();
    sqlx::query!(
        "DELETE FROM visuals WHERE sheet_id = $1 AND NOT (id = ANY($2))",
        sheet_id,
        &ids
    )
    .execute(&mut *tx)
    .await?;
    let positions: Vec<i32> = (0..save.visuals.len())
        .map(|i| i32::try_from(i).unwrap_or(i32::MAX))
        .collect();
    let specs = save
        .visuals
        .iter()
        .map(|v| to_json(&v.spec))
        .collect::<Result<Vec<_>, _>>()?;
    let layouts = save
        .visuals
        .iter()
        .map(|v| to_json(&v.layout))
        .collect::<Result<Vec<_>, _>>()?;
    // A visual id that already belongs to another sheet is not taken over: the update is
    // skipped, the row count comes up short and the save is refused.
    let written = sqlx::query!(
        "INSERT INTO visuals (id, sheet_id, position, spec, layout)
         SELECT t.id, $1, t.position, t.spec, t.layout
         FROM UNNEST($2::uuid[], $3::int[], $4::jsonb[], $5::jsonb[]) AS t(id, position, spec, layout)
         ON CONFLICT (id) DO UPDATE
            SET position = EXCLUDED.position, spec = EXCLUDED.spec, layout = EXCLUDED.layout
            WHERE visuals.sheet_id = EXCLUDED.sheet_id",
        sheet_id,
        &ids,
        &positions,
        &specs,
        &layouts,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if written != ids.len() as u64 {
        return Err(SheetError::Invalid(
            "A visual id belongs to another sheet.".into(),
        ));
    }
    tx.commit().await?;
    Ok(Some(version))
}

/// Puts the project's sheets in the given order (ids not listed keep their position).
pub async fn reorder(pool: &PgPool, project_id: Uuid, ids: &[Uuid]) -> Result<(), SheetError> {
    sqlx::query!(
        "UPDATE sheets s SET position = t.ord::int
         FROM UNNEST($2::uuid[]) WITH ORDINALITY AS t(id, ord)
         WHERE s.id = t.id AND s.project_id = $1",
        project_id,
        ids
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &PgPool, project_id: Uuid, sheet_id: Uuid) -> Result<bool, SheetError> {
    let done = sqlx::query!(
        "DELETE FROM sheets WHERE id = $1 AND project_id = $2",
        sheet_id,
        project_id
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn sharing(
    pool: &PgPool,
    project_id: Uuid,
    sheet_id: Uuid,
) -> Result<Option<SheetSharing>, SheetError> {
    let Some(row) = sqlx::query!(
        r#"SELECT link_audience, link_token, link_expires_at, allow_export, track_views,
                  allow_copilot,
                  (SELECT count(*) FROM sheet_views v WHERE v.sheet_id = s.id) AS "view_count!"
           FROM sheets s WHERE id = $1 AND project_id = $2"#,
        sheet_id,
        project_id
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let members = sqlx::query_as!(
        SheetMember,
        "SELECT u.id AS user_id, u.display_name, u.email
         FROM sheet_members m JOIN users u ON u.id = m.user_id
         WHERE m.sheet_id = $1 ORDER BY m.added_at",
        sheet_id
    )
    .fetch_all(pool)
    .await?;
    Ok(Some(SheetSharing {
        audience: LinkAudience::parse(&row.link_audience)?,
        token: row.link_token,
        expires_at: row.link_expires_at,
        allow_export: row.allow_export,
        track_views: row.track_views,
        allow_copilot: row.allow_copilot,
        members,
        view_count: row.view_count,
    }))
}

/// Applies link settings. `new_token` is used when the sheet has no token yet or the update
/// rotates it; turning the link off forgets the token.
pub async fn set_sharing(
    pool: &PgPool,
    project_id: Uuid,
    sheet_id: Uuid,
    update: &SharingUpdate,
    new_token: &str,
) -> Result<bool, SheetError> {
    let expires_at = update
        .expires_in_days
        .map(|d| Utc::now() + chrono::Duration::days(i64::from(d)));
    let done = sqlx::query!(
        "UPDATE sheets SET
            link_audience = $3::text,
            link_token = CASE WHEN $3::text = 'off' THEN NULL
                              WHEN link_token IS NULL OR $4::bool THEN $5::text
                              ELSE link_token END,
            link_expires_at = CASE WHEN $3::text = 'off' THEN NULL ELSE $6::timestamptz END,
            allow_export = $7, track_views = $8, allow_copilot = $9
         WHERE id = $1 AND project_id = $2",
        sheet_id,
        project_id,
        update.audience.as_str(),
        update.rotate,
        new_token,
        expires_at,
        update.allow_export,
        update.track_views,
        update.allow_copilot,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn add_member(
    pool: &PgPool,
    sheet_id: Uuid,
    user_id: Uuid,
    added_by: Uuid,
) -> Result<(), SheetError> {
    sqlx::query!(
        "INSERT INTO sheet_members (sheet_id, user_id, added_by) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
        sheet_id,
        user_id,
        added_by
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_member(pool: &PgPool, sheet_id: Uuid, user_id: Uuid) -> Result<(), SheetError> {
    sqlx::query!(
        "DELETE FROM sheet_members WHERE sheet_id = $1 AND user_id = $2",
        sheet_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_member(pool: &PgPool, sheet_id: Uuid, user_id: Uuid) -> Result<bool, SheetError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM sheet_members WHERE sheet_id = $1 AND user_id = $2) AS "e!""#,
        sheet_id,
        user_id
    )
    .fetch_one(pool)
    .await?)
}

/// The sheet behind a link, whatever its audience; the caller decides who may open it.
pub async fn by_token(
    pool: &PgPool,
    token: &str,
) -> Result<Option<(SharedSheet, LinkAudience)>, SheetError> {
    let Some(row) = sqlx::query!(
        "SELECT s.id, s.project_id, s.link_audience, s.link_expires_at, s.allow_export,
                s.track_views, p.workspace_id, p.name AS project_name,
                u.display_name AS \"shared_by?\"
         FROM sheets s
         JOIN projects p ON p.id = s.project_id
         LEFT JOIN users u ON u.id = COALESCE(s.updated_by, s.created_by)
         WHERE s.link_token = $1",
        token
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let Some(sheet) = get(pool, row.project_id, row.id).await? else {
        return Ok(None);
    };
    Ok(Some((
        SharedSheet {
            sheet,
            workspace_id: row.workspace_id,
            project_name: row.project_name,
            shared_by: row.shared_by,
            expires_at: row.link_expires_at,
            allow_export: row.allow_export,
            track_views: row.track_views,
        },
        LinkAudience::parse(&row.link_audience)?,
    )))
}

pub async fn record_view(
    pool: &PgPool,
    sheet_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<(), SheetError> {
    sqlx::query!(
        "INSERT INTO sheet_views (sheet_id, user_id) VALUES ($1, $2)",
        sheet_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value, SheetError> {
    serde_json::to_value(value).map_err(|e| SheetError::Corrupt(e.to_string()))
}

fn from_json<T: for<'de> Deserialize<'de>>(
    value: serde_json::Value,
    what: &str,
) -> Result<T, SheetError> {
    serde_json::from_value(value).map_err(|e| SheetError::Corrupt(format!("{what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_visuals::{VisualKind, VisualOptions};

    fn visual(id: u128, x: u16, w: u16) -> Visual {
        Visual {
            id: Uuid::from_u128(id),
            spec: VisualSpec {
                kind: VisualKind::Kpi,
                title: "Net revenue".into(),
                table_id: Uuid::nil(),
                x: None,
                series: None,
                measures: vec![],
                columns: vec![],
                drill: vec![],
                filters: vec![],
                sort: None,
                limit: None,
                options: VisualOptions::default(),
            },
            layout: Layout { x, y: 0, w, h: 2 },
        }
    }

    fn save(visuals: Vec<Visual>) -> SaveSheet {
        SaveSheet {
            version: 1,
            name: "Revenue Review".into(),
            filters: vec![],
            visuals,
        }
    }

    #[test]
    fn validation() {
        assert!(validate(&save(vec![visual(1, 0, 3), visual(2, 3, 9)])).is_ok());
        assert!(
            validate(&save(vec![visual(1, 10, 3)])).is_err(),
            "past column 12"
        );
        assert!(
            validate(&save(vec![visual(1, 0, 0)])).is_err(),
            "zero width"
        );
        assert!(
            validate(&save(vec![visual(1, 0, 3), visual(1, 3, 3)])).is_err(),
            "duplicate id"
        );
        let mut blank = save(vec![]);
        blank.name = "  ".into();
        assert!(validate(&blank).is_err());

        let mut s = save(vec![]);
        s.filters = vec![SheetFilterDef {
            id: Uuid::nil(),
            column: "order_date".into(),
            label: "Fiscal Q3".into(),
            kind: SheetFilterKind::DateRange,
            options: vec![],
            preset: None,
            value: Some(FilterValue::Many(vec![
                Scalar::Text("2026-08-01".into()),
                Scalar::Text("2026-10-31".into()),
            ])),
        }];
        assert!(validate(&s).is_ok());
        s.filters[0].preset = Some(DatePreset::FiscalQuarterToDate);
        assert!(validate(&s).is_ok());
        s.filters[0].kind = SheetFilterKind::Values;
        assert!(validate(&s).is_err(), "only date ranges have presets");
        s.filters[0].kind = SheetFilterKind::DateRange;
        s.filters[0].value = Some(FilterValue::One(Scalar::Number(1.0)));
        assert!(validate(&s).is_err(), "a date range needs two dates");
    }
}
