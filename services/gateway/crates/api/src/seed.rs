//! Demo data for local development: one workspace that looks like the design's
//! "Acme Retail", with a signed-in-able admin, a few teammates, and a generated retail dataset.
//! Idempotent.

use lattice_catalog::{self as catalog, NewSource, SourceStatus};
use lattice_connectors::SourceKind;
use lattice_engine::demo;
use lattice_knowledge::{self as knowledge, NewNote, NoteEdit, NoteKind, Provenance, Viewer};
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
    seed_demo_knowledge(&state.pool, seed).await?;
    Ok(source)
}

/// What the demo seeds into the knowledge graph: (kind, title, team, body, formula, state, local).
type DemoNote = (
    NoteKind,
    &'static str,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
    DemoState,
    bool,
);

#[derive(Clone, Copy, PartialEq)]
enum DemoState {
    Approved,
    Pending,
    Draft,
}

/// A starter knowledge graph shaped like the design's screen 05, in every state: approved notes,
/// proposals waiting for review, a draft, and links to a note nobody has written yet. Seeded
/// through the same create → submit → approve path people use. Nothing here claims to come
/// from the AI (phase 4): proposals are marked as demo content.
pub async fn seed_demo_knowledge(pool: &PgPool, seed: &DemoSeed) -> Result<(), ApiError> {
    use DemoState::*;
    use NoteKind::*;
    let notes: [DemoNote; 17] = [
        (
            Entity,
            "Customer",
            Some("Data platform"),
            "A person or business that has bought from Acme Retail: one customer, even when they \
          appear in several systems. Places [[Order]]s; counted by [[Active customer]] and \
          [[Churned customer]].\n\nMapped to the `customers` table.",
            None,
            Approved,
            false,
        ),
        (
            Entity,
            "Order",
            Some("Data platform"),
            "A purchase, online or in store, by a [[Customer]] at a [[Store]], in a \
          [[Product category]]. Only [[Settled orders only|settled]] orders count toward \
          [[Net revenue]].\n\nMapped to the `orders` table.",
            None,
            Approved,
            false,
        ),
        (
            Entity,
            "Store",
            Some("Retail ops"),
            "A shop or the online store, located in a [[Region]].\n\nMapped to the `stores` table.",
            None,
            Approved,
            false,
        ),
        (
            Entity,
            "Refund",
            Some("Finance"),
            "Money returned for an [[Order]]: an order whose status is `refunded`. Excluded from \
          [[Net revenue]] by [[Settled orders only]].",
            None,
            Approved,
            false,
        ),
        (
            Dimension,
            "Region",
            Some("Retail ops"),
            "AMER, EMEA or APAC: the region of the [[Store]] an [[Order]] was placed at.",
            None,
            Approved,
            false,
        ),
        (
            Dimension,
            "Product category",
            Some("Merchandising"),
            "The category of what an [[Order]] bought: Home & kitchen, Outdoor, Apparel, Electronics, \
          Beauty, Toys, Grocery, Sports.",
            None,
            Approved,
            false,
        ),
        (
            Metric,
            "Net revenue",
            Some("Finance"),
            "Revenue from [[Order]]s that are [[Settled orders only|settled]] — refunded and pending \
          orders don't count. Reported by the [[Fiscal calendar]]. Also called [[Sales]] or \
          [[Turnover]].",
            Some("SUM(order.amount) WHERE order.status = 'settled'"),
            Approved,
            false,
        ),
        (
            Metric,
            "Average order value",
            Some("Finance"),
            "[[Net revenue]] divided by the number of settled [[Order]]s.",
            Some("SUM(order.amount) / COUNT(order) WHERE order.status = 'settled'"),
            Approved,
            false,
        ),
        (
            Metric,
            "Active customer",
            Some("Growth"),
            "A [[Customer]] with a settled [[Order]] in the last 90 days.",
            Some(
                "COUNT DISTINCT order.customer WHERE order.status = 'settled' AND order.date >= today - 90 days",
            ),
            Approved,
            false,
        ),
        (
            Rule,
            "Settled orders only",
            Some("Finance"),
            "Only orders with status `settled` count toward revenue metrics such as [[Net revenue]]; \
          `pending` and `refunded` ([[Refund]]) orders don't.",
            None,
            Approved,
            false,
        ),
        (
            Rule,
            "Fiscal calendar",
            Some("Finance"),
            "The fiscal year starts on 1 February, so fiscal Q3 runs August–October. Period \
          comparisons of [[Net revenue]] use fiscal quarters.",
            None,
            Approved,
            false,
        ),
        (
            Term,
            "Sales",
            None,
            "Another word for [[Net revenue]].",
            None,
            Approved,
            false,
        ),
        (
            Term,
            "Turnover",
            None,
            "Another word for [[Net revenue]].",
            None,
            Approved,
            false,
        ),
        (
            Metric,
            "Churned customer",
            Some("Growth"),
            "A [[Customer]] whose last settled [[Order]] was more than 180 days ago.",
            Some("COUNT customer WHERE days_since(last settled order.date) > 180"),
            Pending,
            false,
        ),
        (
            Rule,
            "Exclude internal accounts",
            Some("Data platform"),
            "Customers flagged `is_internal` (staff and test accounts) are left out of every \
          [[Customer]] metric, such as [[Active customer]].",
            None,
            Pending,
            false,
        ),
        (
            Metric,
            "Refund rate",
            Some("Finance"),
            "The share of [[Order]]s that end as a [[Refund]]. About 3 % of orders in the demo data \
          have status `refunded`.",
            Some("COUNT(order WHERE order.status = 'refunded') / COUNT(order)"),
            Draft,
            false,
        ),
        (
            Fact,
            "Board pack",
            None,
            "The [[Revenue Review]] sheet goes to the board each quarter, filtered to the fiscal \
          quarter from the [[Fiscal calendar]].",
            None,
            Approved,
            true,
        ),
    ];
    let viewer = Viewer {
        user_id: seed.admin_id,
        workspace_id: seed.workspace_id,
        workspace_role: WorkspaceRole::Admin,
        projects: lattice_project::roles_for_user(pool, seed.workspace_id, seed.admin_id).await?,
    };
    for (kind, title, team, body, formula, state, local) in notes {
        let new = NewNote {
            kind,
            title: title.into(),
            body: body.into(),
            definition: formula.map(|f| json!({ "formula": f })),
            project_id: local.then_some(seed.revenue_project),
            owner_team: team.map(Into::into),
            submit: Some(state != Draft),
        };
        let provenance = Provenance {
            author: "system".into(),
            value: json!({ "source": "demo" }),
        };
        let id = knowledge::create(pool, &viewer, &new, provenance).await?;
        if state == Approved {
            knowledge::approve(pool, &viewer, id).await?;
        }
    }
    // A proposed change waiting on an approved metric: shorten the active window.
    let active = knowledge::graph(pool, &viewer, None)
        .await?
        .nodes
        .into_iter()
        .find(|n| n.title == "Active customer")
        .map(|n| n.id);
    if let Some(id) = active {
        knowledge::save_draft(
            pool,
            &viewer,
            id,
            &NoteEdit {
                title: "Active customer".into(),
                body: "A [[Customer]] with a settled [[Order]] in the last 60 days.".into(),
                definition: Some(json!({ "formula": "COUNT DISTINCT order.customer WHERE order.status = 'settled' AND order.date >= today - 60 days" })),
                change_note: Some("Growth reviews activity monthly; 90 days lags behind.".into()),
            },
        )
        .await?;
        knowledge::submit(pool, &viewer, id).await?;
    }
    Ok(())
}

/// "Revenue Review" (screen 06 of the design), "Regional ops" and "Order explorer" (a million
/// points on a WebGL scatter plot).
async fn seed_demo_sheets(state: &AppState, seed: &DemoSeed, orders: Uuid) -> Result<(), ApiError> {
    let fiscal_q3 = json!({
        "id": Uuid::new_v4(), "column": "order_date", "label": "Fiscal Q3 · Aug–Oct",
        "kind": "date_range", "value": ["2026-08-01", "2026-10-31"]
    });
    let regions = json!({
        "id": Uuid::new_v4(), "column": "region", "label": "Region", "kind": "values",
        "options": ["AMER", "EMEA", "APAC"]
    });
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
