//! Demo data for local development: one workspace that looks like the design's
//! "Acme Retail", with a signed-in-able admin, a few teammates, and a generated retail dataset.
//! Idempotent.

use lattice_catalog::{self as catalog, NewSource, SourceStatus};
use lattice_connectors::SourceKind;
use lattice_engine::demo;
use lattice_project::NewProject;
use lattice_security::ProjectRole;
use lattice_security::WorkspaceRole;
use lattice_security::identity::{self, NewAccount};
use lattice_security::password::hash_password;
use serde_json::{Value, json};
use sqlx::PgPool;

use uuid::Uuid;

use crate::error::ApiError;
use crate::sources;
use crate::state::AppState;

pub const DEMO_EMAIL: &str = "demo@lattice.dev";
pub const DEMO_PASSWORD: &str = "lattice-demo";

/// What a fresh seed created, for seeding data into it.
#[derive(Debug, Clone, Copy)]
pub struct DemoSeed {
    pub admin_id: Uuid,
    pub workspace_id: Uuid,
    /// "Revenue Analytics", which gets the demo dataset.
    pub revenue_project: Uuid,
}

/// Returns `None` when the demo account already exists (nothing is changed).
pub async fn seed_demo(pool: &PgPool) -> Result<Option<DemoSeed>, ApiError> {
    if identity::find_password_credentials(pool, DEMO_EMAIL)
        .await?
        .is_some()
    {
        return Ok(None);
    }

    let hash = tokio::task::spawn_blocking(|| hash_password(DEMO_PASSWORD)).await??;
    let admin = identity::create_account_with_workspace(
        pool,
        NewAccount {
            email: DEMO_EMAIL,
            display_name: "Demo Admin",
            password_hash: Some(&hash),
            workspace_name: "Acme Retail",
        },
    )
    .await?;

    // Teammates have no password, so nobody can sign in as them.
    let priya = identity::add_member_without_login(
        pool,
        admin.workspace_id,
        "priya@acme.example",
        "Priya R.",
        WorkspaceRole::Member,
    )
    .await?;
    let jonas = identity::add_member_without_login(
        pool,
        admin.workspace_id,
        "jonas@acme.example",
        "Jonas M.",
        WorkspaceRole::Member,
    )
    .await?;

    let projects = [
        (
            "Revenue Analytics",
            "Net revenue, refunds and regional performance for Finance leadership.",
            vec![(priya, ProjectRole::Editor), (jonas, ProjectRole::Editor)],
        ),
        (
            "Customer 360",
            "Lifecycle, churn and cohort behaviour across CRM and orders.",
            vec![(priya, ProjectRole::Viewer)],
        ),
        (
            "Supply Chain Ops",
            "Inventory turns, supplier lead times and stock-outs by warehouse.",
            vec![(jonas, ProjectRole::Admin)],
        ),
    ];
    // Created last-first so "Revenue Analytics" is the most recently updated, as in the design.
    let mut revenue_project = Uuid::nil();
    for (name, description, members) in projects.into_iter().rev() {
        let id = lattice_project::create(
            pool,
            NewProject {
                workspace_id: admin.workspace_id,
                created_by: admin.user_id,
                name,
                description,
            },
        )
        .await?;
        for (user_id, role) in members {
            lattice_project::add_member(pool, id, user_id, role).await?;
        }
        revenue_project = id;
    }
    Ok(Some(DemoSeed {
        admin_id: admin.user_id,
        workspace_id: admin.workspace_id,
        revenue_project,
    }))
}

/// Adds the generated "Acme Retail orders" dataset to the demo's Revenue Analytics project,
/// with sheets shaped like the design's.
pub async fn seed_demo_data(
    state: &AppState,
    seed: &DemoSeed,
    orders: u64,
) -> Result<Uuid, ApiError> {
    let source = generated_source(
        state,
        seed.revenue_project,
        seed.admin_id,
        "Acme Retail orders (demo)",
        orders,
    )
    .await?;
    let table = sqlx::query_scalar!(
        "SELECT id FROM source_tables WHERE source_id = $1 AND table_name = 'orders'",
        source
    )
    .fetch_one(&state.pool)
    .await?;
    seed_demo_sheets(state, seed, table).await?;
    Ok(source)
}

/// "Revenue Review" (screen 06 of the design), "Regional ops" and "Order explorer" (a million
/// points on a WebGL scatter plot).
async fn seed_demo_sheets(state: &AppState, seed: &DemoSeed, orders: Uuid) -> Result<(), ApiError> {
    let fiscal_q3 = json!({
        "id": Uuid::new_v4(), "column": "order_date", "label": "Fiscal Q3 · Aug–Oct",
        "kind": "date_range", "value": ["2026-08-01", "2026-10-31"]
    });
    let regions =
        json!({ "id": Uuid::new_v4(), "column": "region", "label": "Region", "kind": "values" });
    let sum = json!({ "column": "amount", "agg": "sum" });
    let dim = |column: &str| json!({ "column": column, "bucket": null });
    let kpi = |title: &str, measure: Value, format: &str| {
        json!({ "kind": "kpi", "title": title, "measures": [measure],
                "options": { "compare": "order_date", "format": format } })
    };
    let sheets = [
        (
            "Revenue Review",
            vec![fiscal_q3.clone(), regions.clone()],
            vec![
                (kpi("Net revenue", sum.clone(), "currency"), [0, 0, 3, 2]),
                (
                    kpi("Orders", json!({ "agg": "count" }), "number"),
                    [3, 0, 3, 2],
                ),
                (
                    kpi(
                        "Avg order value",
                        json!({ "column": "amount", "agg": "avg" }),
                        "currency",
                    ),
                    [6, 0, 3, 2],
                ),
                (
                    kpi(
                        "Active customers",
                        json!({ "column": "customer_id", "agg": "approx_count_distinct" }),
                        "number",
                    ),
                    [9, 0, 3, 2],
                ),
                (
                    json!({ "kind": "area", "title": "Net revenue by week",
                         "x": { "column": "order_date", "bucket": "week" }, "measures": [sum.clone()],
                         "options": { "format": "currency" } }),
                    [0, 2, 12, 4],
                ),
                (
                    json!({ "kind": "bar", "title": "Net revenue by region", "x": dim("region"),
                         "drill": [dim("category"), dim("channel")], "measures": [sum.clone()],
                         "options": { "format": "currency" } }),
                    [0, 6, 6, 4],
                ),
                (
                    json!({ "kind": "table", "title": "Top categories", "x": dim("category"),
                         "measures": [sum.clone(), { "agg": "count" }],
                         "sort": { "by": "value", "dir": "desc" }, "limit": 10,
                         "options": { "format": "currency" } }),
                    [6, 6, 6, 4],
                ),
            ],
        ),
        (
            "Regional ops",
            vec![regions.clone()],
            vec![
                (
                    json!({ "kind": "slicer", "title": "Region", "x": dim("region") }),
                    [0, 0, 3, 4],
                ),
                (
                    json!({ "kind": "heatmap", "title": "Revenue by category and region",
                         "x": dim("category"), "series": dim("region"), "measures": [sum.clone()],
                         "options": { "format": "currency" } }),
                    [3, 0, 9, 4],
                ),
                (
                    json!({ "kind": "donut", "title": "Channel mix", "x": dim("channel"),
                         "measures": [sum.clone()], "options": { "format": "currency" } }),
                    [0, 4, 4, 4],
                ),
                (
                    json!({ "kind": "histogram", "title": "Order value distribution",
                         "x": { "column": "amount", "bucket": null, "bin": 50.0 } }),
                    [4, 4, 8, 4],
                ),
                (
                    json!({ "kind": "pivot", "title": "Monthly revenue by region",
                         "x": { "column": "order_date", "bucket": "month" }, "series": dim("region"),
                         "measures": [sum.clone()], "options": { "format": "currency" } }),
                    [0, 8, 12, 5],
                ),
            ],
        ),
        (
            "Order explorer",
            vec![],
            vec![
                (
                    json!({ "kind": "scatter", "title": "Every order: amount over time",
                         "x": dim("order_date"), "columns": ["amount"], "series": dim("region"),
                         "limit": 1_000_000 }),
                    [0, 0, 12, 6],
                ),
                (
                    json!({ "kind": "table", "title": "Orders",
                         "columns": ["order_id", "order_date", "region", "category", "status", "amount"] }),
                    [0, 6, 12, 5],
                ),
            ],
        ),
    ];
    for (name, filters, visuals) in sheets {
        let filters: Vec<lattice_sheets::SheetFilterDef> =
            serde_json::from_value(Value::Array(filters))?;
        let id = lattice_sheets::create(
            &state.pool,
            seed.revenue_project,
            name,
            &filters,
            seed.admin_id,
        )
        .await?;
        let visuals = visuals
            .into_iter()
            .map(|(mut spec, [x, y, w, h])| {
                spec["table_id"] = json!(orders);
                Ok(lattice_sheets::Visual {
                    id: Uuid::new_v4(),
                    spec: serde_json::from_value(spec)?,
                    layout: lattice_sheets::Layout { x, y, w, h },
                })
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()?;
        let save = lattice_sheets::SaveSheet {
            version: 1,
            name: name.into(),
            filters,
            visuals,
        };
        lattice_sheets::save(&state.pool, seed.revenue_project, id, &save, seed.admin_id).await?;
    }
    Ok(())
}

/// Creates a "Benchmark" project for the demo admin with `orders` generated rows (the phase 1
/// exit criterion is measured on 50M). Returns the project id.
pub async fn seed_bench(state: &AppState, orders: u64) -> Result<Uuid, ApiError> {
    let admin = sqlx::query!(
        "SELECT u.id, m.workspace_id FROM users u JOIN workspace_members m ON m.user_id = u.id
         WHERE u.email = $1 ORDER BY m.created_at LIMIT 1",
        DEMO_EMAIL
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::BadRequest("Run the demo seed first.".into()))?;
    let name = format!("Benchmark · {} orders", human_count(orders));
    let project = lattice_project::create(
        &state.pool,
        NewProject {
            workspace_id: admin.workspace_id,
            created_by: admin.id,
            name: &name,
            description: "Generated data for the engine and end-to-end query benchmarks.",
        },
    )
    .await?;
    generated_source(state, project, admin.id, "Benchmark orders", orders).await?;
    Ok(project)
}

async fn generated_source(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
    name: &str,
    orders: u64,
) -> Result<Uuid, ApiError> {
    let source_id = catalog::insert_source(
        &state.pool,
        NewSource {
            project_id,
            kind: SourceKind::Demo,
            name,
            config: serde_json::json!({ "rows": orders }),
            created_by: user_id,
        },
    )
    .await?;
    let db = sources::project_db(state, project_id).await?;
    let schema = sources::extract_schema(source_id);
    let customers = (orders / 20).max(1000);
    db.create_table_as(schema.clone(), "stores".into(), demo::STORES.into())
        .await?;
    db.create_table_as(
        schema.clone(),
        "customers".into(),
        demo::customers_sql(customers),
    )
    .await?;
    db.create_table_as(
        schema.clone(),
        "orders".into(),
        demo::orders_sql(orders, customers),
    )
    .await?;
    let tables = db.introspect("project".into(), Some(schema)).await?;
    sources::sync(state, &db, source_id, SourceKind::Demo, &tables).await?;
    catalog::set_status(&state.pool, source_id, SourceStatus::Ready, None).await?;
    Ok(source_id)
}

fn human_count(n: u64) -> String {
    match n {
        n if n >= 1_000_000 && n % 1_000_000 == 0 => format!("{}M", n / 1_000_000),
        n if n >= 1_000 && n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}
