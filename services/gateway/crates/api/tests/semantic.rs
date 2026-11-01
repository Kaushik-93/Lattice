//! The semantic model end to end (docs/adr/0006): the demo's knowledge, bound to the demo's
//! tables, compiles to SQL whose answers match hand-written queries.

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use chrono::NaiveDate;
use common::*;
use lattice_api::state::AppState;
use lattice_planner::ParamValue;
use lattice_semantic::ast::Path;
use lattice_semantic::{Model, SemDimension, SemanticQuery, compile};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Demo {
    h: Harness,
    cookie: String,
    project: Uuid,
    workspace: Uuid,
}

async fn demo(pool: PgPool) -> Demo {
    let h = harness(pool.clone()).await;
    let seed = lattice_api::seed::seed_demo(&pool).await.unwrap().unwrap();
    lattice_api::seed::seed_demo_data(&h.state, &seed, 20_000)
        .await
        .unwrap();
    let cookie = json_call(
        &h.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": lattice_api::seed::DEMO_EMAIL, "password": lattice_api::seed::DEMO_PASSWORD })),
    )
    .await
    .cookie
    .unwrap();
    Demo {
        h,
        cookie,
        project: seed.revenue_project,
        workspace: seed.workspace_id,
    }
}

fn named<'a>(list: &'a Value, title: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|v| v["title"] == title)
        .unwrap_or_else(|| panic!("no {title:?} in {list}"))
}

fn id_of(model: &Model, title: &str) -> Uuid {
    model.metric(title).unwrap().meta.note_id
}

/// The rows of a query as sorted text, so two queries can be compared.
async fn rows(state: &AppState, project: Uuid, sql: &str, params: Vec<ParamValue>) -> String {
    let db = lattice_api::sources::project_db(state, project)
        .await
        .unwrap();
    let wrapped = format!(
        "SELECT string_agg(CAST(t AS VARCHAR), ' | ' ORDER BY CAST(t AS VARCHAR)) AS v FROM ({sql}) t"
    );
    let out = db
        .query(wrapped, params, &CancellationToken::new())
        .await
        .unwrap();
    let col = out.batches[0].column(0);
    arrow::array::cast::as_string_array(col).value(0).to_owned()
}

async fn compiled_rows(d: &Demo, model: &Model, q: &SemanticQuery) -> String {
    let c = compile(model, q).unwrap();
    let sql = c.query.sql.trim_end().to_owned();
    rows(&d.h.state, d.project, &sql, c.query.params).await
}

fn by_region(metrics: Vec<Uuid>) -> SemanticQuery {
    SemanticQuery {
        metrics,
        dimensions: vec![SemDimension {
            path: Path {
                entity: "order".into(),
                column: "region".into(),
            },
            bucket: None,
            bin: None,
        }],
        filters: vec![],
        context_filters: vec![],
        sort: vec![],
        limit: None,
        today: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
    }
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn the_demo_model_checks_out(pool: PgPool) {
    let d = demo(pool).await;
    let path = format!("/api/projects/{}/model", d.project);
    let m = json_call(&d.h.app, "GET", &path, Some(&d.cookie), None)
        .await
        .body;
    assert_eq!(m["fiscal_year_start_month"], 2);

    let order = named(&m["entities"], "Order");
    assert_eq!(order["ident"], "order");
    assert_eq!(order["table"]["name"], "orders");
    assert_eq!(order["problem"], Value::Null);
    assert!(
        named(&m["entities"], "Refund")["problem"]
            .as_str()
            .unwrap()
            .contains("Not connected")
    );

    for title in ["Net revenue", "Average order value", "Active customer"] {
        let metric = named(&m["metrics"], title);
        assert_eq!(metric["problem"], Value::Null, "{title}: {metric}");
        assert_eq!(metric["grain"], "order");
        assert_eq!(metric["standing"], "approved");
    }
    let aov = named(&m["metrics"], "Average order value");
    let net = named(&m["metrics"], "Net revenue");
    assert!(aov["uses"].as_array().unwrap().contains(&net["note_id"]));
    // A draft that has never been approved is usable, marked unapproved.
    let refund_rate = named(&m["metrics"], "Refund rate");
    assert_eq!(refund_rate["standing"], "unapproved");
    assert_eq!(refund_rate["problem"], Value::Null);
    assert!(
        named(&m["metrics"], "Churned customer")["problem"]
            .as_str()
            .unwrap()
            .contains("no formula")
    );

    for list in ["dimensions", "rules", "relationships"] {
        for item in m[list].as_array().unwrap() {
            assert_eq!(item["problem"], Value::Null, "{list}: {item}");
        }
    }
    assert_eq!(
        named(&m["rules"], "Exclude internal accounts")["always"],
        true
    );

    // Unbinding Store breaks what reads it; binding it again restores it.
    let store = named(&m["entities"], "Store");
    let binding = format!(
        "/api/projects/{}/bindings/{}",
        d.project,
        store["note_id"].as_str().unwrap()
    );
    let table_id = store["table"]["table_id"].clone();
    let after = json_call(&d.h.app, "DELETE", &binding, Some(&d.cookie), None).await;
    assert_eq!(after.status, StatusCode::OK);
    assert!(
        named(&after.body["relationships"], "Order sold at Store")["problem"]
            .as_str()
            .unwrap()
            .contains("isn't connected")
    );
    let again = json_call(
        &d.h.app,
        "PUT",
        &binding,
        Some(&d.cookie),
        Some(json!({ "table_id": table_id })),
    )
    .await;
    assert_eq!(
        named(&again.body["relationships"], "Order sold at Store")["problem"],
        Value::Null
    );

    // Only entities bind, and only to this project's tables.
    let metric_binding = format!(
        "/api/projects/{}/bindings/{}",
        d.project,
        net["note_id"].as_str().unwrap()
    );
    let reply = json_call(
        &d.h.app,
        "PUT",
        &metric_binding,
        Some(&d.cookie),
        Some(json!({ "table_id": table_id })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = json_call(
        &d.h.app,
        "PUT",
        &binding,
        Some(&d.cookie),
        Some(json!({ "table_id": Uuid::new_v4() })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn compiled_metrics_match_hand_written_sql(pool: PgPool) {
    let d = demo(pool).await;
    let model = lattice_api::semantic::load(&d.h.state, d.workspace, d.project)
        .await
        .unwrap()
        .model;
    let orders = &model
        .entity("order")
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .reference;
    let customers = &model
        .entity("customer")
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .reference;

    let q = by_region(vec![
        id_of(&model, "Net revenue"),
        id_of(&model, "Average order value"),
        id_of(&model, "Active customer"),
    ]);
    let expected = format!(
        "SELECT region,
                sum(amount) FILTER (WHERE status = 'settled') AS net_revenue,
                CAST(sum(amount) FILTER (WHERE status = 'settled') AS DOUBLE)
                    / count(*) FILTER (WHERE status = 'settled') AS average_order_value,
                count(DISTINCT customer_id) FILTER (WHERE status = 'settled'
                    AND order_date >= DATE '2026-11-01' - INTERVAL 90 DAY) AS active_customer
         FROM {orders} GROUP BY region"
    );
    let want = rows(&d.h.state, d.project, &expected, vec![]).await;
    assert!(want.contains("AMER"), "{want}");
    assert_eq!(compiled_rows(&d, &model, &q).await, want);

    // A pending standing rule doesn't change approved numbers until it's approved…
    let rule = model
        .rule("Exclude internal accounts")
        .unwrap()
        .meta
        .note_id;
    let approve = format!("/api/knowledge/notes/{rule}/approve");
    let reply = json_call(&d.h.app, "POST", &approve, Some(&d.cookie), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    // …after which every query reaching customers leaves internal accounts out.
    let model = lattice_api::semantic::load(&d.h.state, d.workspace, d.project)
        .await
        .unwrap()
        .model;
    let q = by_region(vec![id_of(&model, "Net revenue")]);
    let expected = format!(
        "SELECT o.region, sum(o.amount) FILTER (WHERE o.status = 'settled') AS net_revenue
         FROM {orders} o LEFT JOIN {customers} c ON o.customer_id = c.customer_id
         WHERE COALESCE(c.is_internal = FALSE, TRUE)
         GROUP BY o.region"
    );
    let excluded = rows(&d.h.state, d.project, &expected, vec![]).await;
    assert_ne!(
        excluded, want,
        "the demo has internal accounts to leave out"
    );
    assert_eq!(compiled_rows(&d, &model, &q).await, excluded);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn definitions_are_checked_when_saved(pool: PgPool) {
    let h = harness(pool).await;
    let cookie = signup(&h.app, "ada@example.com").await;
    let create = |kind: &str, definition: Value| json!({ "kind": kind, "title": format!("A {kind}"), "body": "", "definition": definition });
    for (kind, definition, message) in [
        (
            "metric",
            json!({ "formula": "SUM(order.amount" }),
            "Formula",
        ),
        (
            "rule",
            json!({ "condition": "order.status ==" }),
            "Condition",
        ),
        (
            "rule",
            json!({ "fiscal_year_start_month": 13 }),
            "month 1–12",
        ),
        (
            "relationship",
            json!({ "from": "order.customer_id" }),
            "\"to\"",
        ),
        ("dimension", json!({ "path": "region" }), "entity.column"),
        (
            "metric",
            json!({ "formula": "COUNT(order)", "cases": [{ "name": "x", "rows": {} }] }),
            "expect",
        ),
        (
            "rule",
            json!({ "condition": "order.amount > 0",
                    "cases": [{ "name": "x", "rows": {}, "expect": 1 }] }),
            "count",
        ),
    ] {
        let reply = json_call(
            &h.app,
            "POST",
            "/api/knowledge/notes",
            Some(&cookie),
            Some(create(kind, definition)),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{kind}");
        let text = reply.body["error"]["message"].as_str().unwrap();
        assert!(text.contains(message), "{kind}: {text}");
    }
    let ok = json_call(
        &h.app,
        "POST",
        "/api/knowledge/notes",
        Some(&cookie),
        Some(create("metric", json!({ "formula": "COUNT(order)" }))),
    )
    .await;
    assert_eq!(ok.status, StatusCode::CREATED);
    // Saving a draft is checked too.
    let id = ok.body["id"].as_str().unwrap();
    let reply = json_call(
        &h.app,
        "PUT",
        &format!("/api/knowledge/notes/{id}/draft"),
        Some(&cookie),
        Some(json!({ "title": "A metric", "body": "", "definition": { "formula": "COUNT(" } })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

fn results(events: &[Event]) -> Vec<(&lattice_api::dto::ResultHeader, usize)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Result(h, rows) => Some((h, *rows)),
            _ => None,
        })
        .collect()
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn visuals_chart_metrics(pool: PgPool) {
    use futures_util::SinkExt as _;
    use tokio_tungstenite::tungstenite::Message;

    let d = demo(pool).await;
    let path = format!("/api/projects/{}/model", d.project);
    let m = json_call(&d.h.app, "GET", &path, Some(&d.cookie), None)
        .await
        .body;
    let orders = named(&m["entities"], "Order")["table"]["table_id"].clone();
    let net = named(&m["metrics"], "Net revenue")["note_id"].clone();
    let refund_rate = named(&m["metrics"], "Refund rate")["note_id"].clone();
    let metric = |id: &Value| json!({ "agg": "sum", "metric": id });
    let (bar, kpi, mixed, joined) = (
        Uuid::from_u128(1),
        Uuid::from_u128(2),
        Uuid::from_u128(3),
        Uuid::from_u128(4),
    );
    let visuals = json!([
        { "id": bar, "spec": { "kind": "bar", "title": "Net revenue by region", "table_id": orders,
          "x": { "column": "region" }, "measures": [metric(&net)] } },
        { "id": kpi, "spec": { "kind": "kpi", "title": "Refund rate", "table_id": orders,
          "measures": [metric(&refund_rate)], "options": { "compare": "order_date" } } },
        { "id": mixed, "spec": { "kind": "bar", "title": "Mixed", "table_id": orders,
          "x": { "column": "region" }, "measures": [metric(&net), { "column": "amount", "agg": "sum" }] } },
        // A related entity's column, through "Order placed by Customer".
        { "id": joined, "spec": { "kind": "column", "title": "By segment", "table_id": orders,
          "x": { "column": "customer.segment" }, "measures": [metric(&net)] } },
    ]);
    let context = json!({ "filters": [
        { "column": "order_date", "op": "between", "value": ["2026-08-01", "2026-10-31"] }
    ] });
    let addr = serve(d.h.state.clone()).await;
    let mut ws = connect(addr, &d.cookie).await;
    let mut run = async |batch_id: u32, visuals: &Value, context: &Value| {
        let msg = json!({ "type": "sheet_batch", "batch_id": batch_id, "project_id": d.project,
                          "context": context, "visuals": visuals });
        ws.send(Message::text(msg.to_string())).await.unwrap();
        until_done(&mut ws, batch_id).await
    };

    let events = run(1, &visuals, &context).await;
    let out = results(&events);
    let find = |id: &str| {
        out.iter()
            .find(|(h, _)| h.query_id == id)
            .unwrap_or_else(|| panic!("no result for {id}"))
    };
    let (h, rows) = find(&bar.to_string());
    assert_eq!(*rows, 3);
    let names: Vec<&str> = h.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["region", "net_revenue"]);
    assert!(h.unapproved.is_empty(), "every definition is approved");
    let (h, rows) = find(&kpi.to_string());
    assert_eq!(*rows, 1);
    assert_eq!(h.unapproved.len(), 1);
    assert_eq!(h.unapproved[0].title, "Refund rate");
    let (h, _) = find(&format!("{kpi}:compare"));
    assert_eq!(h.unapproved[0].title, "Refund rate");
    let (h, rows) = find(&joined.to_string());
    assert_eq!(*rows, 2, "consumer and business");
    assert_eq!(h.columns[0].name, "segment");
    let error = events
        .iter()
        .find_map(|e| match e {
            Event::Error(v) if v["query_id"] == mixed.to_string() => Some(v),
            _ => None,
        })
        .unwrap();
    assert!(
        error["message"].as_str().unwrap().contains("not both"),
        "{error}"
    );

    // Sheet filters on the grain's columns apply.
    let one_region = json!({ "filters": [{ "column": "region", "op": "in", "value": ["EMEA"] }] });
    let bar_only = json!([visuals[0].clone()]);
    let events = run(2, &bar_only, &one_region).await;
    let (h, rows) = results(&events)[0];
    assert_eq!(rows, 1);
    let fingerprint = h.fingerprint.clone();
    let events = run(3, &bar_only, &one_region).await;
    assert!(results(&events)[0].0.cached);

    // A new approved version of the metric is a new result, not a cached one.
    let net_id = net.as_str().unwrap();
    let edit = json!({ "title": "Net revenue", "body": "Every order's amount.",
                       "definition": { "formula": "SUM(order.amount)" } });
    for (method, action, body) in [
        ("PUT", "draft", Some(edit)),
        ("POST", "submit", None),
        ("POST", "approve", None),
    ] {
        let path = format!("/api/knowledge/notes/{net_id}/{action}");
        let reply = json_call(&d.h.app, method, &path, Some(&d.cookie), body).await;
        assert_eq!(reply.status, StatusCode::OK, "{action}: {}", reply.body);
    }
    let events = run(4, &bar_only, &one_region).await;
    let (h, _) = results(&events)[0];
    assert!(!h.cached);
    assert_ne!(h.fingerprint, fingerprint);
}

async fn cases(d: &Demo, id: &str) -> Value {
    let path = format!("/api/knowledge/notes/{id}/cases");
    let reply = json_call(&d.h.app, "POST", &path, Some(&d.cookie), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn example_cases_gate_approval(pool: PgPool) {
    let d = demo(pool).await;
    let graph = json_call(
        &d.h.app,
        "GET",
        "/api/knowledge/graph",
        Some(&d.cookie),
        None,
    )
    .await
    .body;
    let id = |title: &str| {
        named(&graph["nodes"], title)["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    // Every seeded case passes, the proposed 60-day window's included.
    for title in [
        "Net revenue",
        "Average order value",
        "Settled orders only",
        "Exclude internal accounts",
        "Refund rate",
        "Active customer",
    ] {
        let run = cases(&d, &id(title)).await;
        let results = run["results"].as_array().unwrap();
        assert!(!results.is_empty(), "{title}");
        for r in results {
            assert_eq!(r["passed"], true, "{title}: {r}");
        }
    }
    let active = cases(&d, &id("Active customer")).await;
    assert_eq!(active["version"], 2, "the open change's cases");
    assert_eq!(active["results"][0]["actual"], 1.0);

    // A definition whose example fails can't be approved; fixed, it can.
    let rows = json!([
        { "amount": 100, "status": "settled" },
        { "amount": 40, "status": "refunded" }
    ]);
    let new = json!({ "kind": "metric", "title": "Settled revenue", "body": "", "submit": true,
        "definition": { "formula": "SUM(order.amount)",
            "cases": [{ "name": "Refunds don't count", "rows": { "order": rows }, "expect": 100 }] } });
    let note = json_call(
        &d.h.app,
        "POST",
        "/api/knowledge/notes",
        Some(&d.cookie),
        Some(new),
    )
    .await
    .body;
    let note_id = note["id"].as_str().unwrap();
    let run = cases(&d, note_id).await;
    assert_eq!(run["results"][0]["passed"], false);
    assert_eq!(run["results"][0]["actual"], 140.0);
    let approve = format!("/api/knowledge/notes/{note_id}/approve");
    let reply = json_call(&d.h.app, "POST", &approve, Some(&d.cookie), None).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert!(
        reply.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Refunds don't count")
    );
    let fixed = json!({ "title": "Settled revenue", "body": "",
        "definition": { "formula": "SUM(order.amount) WHERE [[Settled orders only]]",
            "cases": [{ "name": "Refunds don't count", "rows": { "order": rows }, "expect": 100 }] } });
    for (method, action, body) in [("PUT", "draft", Some(fixed)), ("POST", "submit", None)] {
        let path = format!("/api/knowledge/notes/{note_id}/{action}");
        let reply = json_call(&d.h.app, method, &path, Some(&d.cookie), body).await;
        assert_eq!(reply.status, StatusCode::OK, "{action}: {}", reply.body);
    }
    let reply = json_call(&d.h.app, "POST", &approve, Some(&d.cookie), None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["state"], "approved");

    // Rows that name something that isn't an entity explain themselves.
    let odd = json!({ "kind": "metric", "title": "Odd", "body": "",
        "definition": { "formula": "COUNT(order)",
            "cases": [{ "name": "Typo", "rows": { "ordr": [{ "amount": 1 }] }, "expect": 1 }] } });
    let note = json_call(
        &d.h.app,
        "POST",
        "/api/knowledge/notes",
        Some(&d.cookie),
        Some(odd),
    )
    .await
    .body;
    let run = cases(&d, note["id"].as_str().unwrap()).await;
    assert!(
        run["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("isn't an entity")
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn impact_lists_dependents_and_visuals(pool: PgPool) {
    let d = demo(pool).await;
    let graph = json_call(
        &d.h.app,
        "GET",
        "/api/knowledge/graph",
        Some(&d.cookie),
        None,
    )
    .await
    .body;
    let impact = |title: &str| {
        let id = named(&graph["nodes"], title)["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (app, cookie) = (d.h.app.clone(), d.cookie.clone());
        async move {
            json_call(
                &app,
                "GET",
                &format!("/api/knowledge/notes/{id}/impact"),
                Some(&cookie),
                None,
            )
            .await
            .body
        }
    };
    let titles = |v: &Value, key: &str, field: &str| -> Vec<String> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x[field].as_str().unwrap().to_owned())
            .collect()
    };

    let settled = impact("Settled orders only").await;
    let dependents = titles(&settled, "dependents", "title");
    for t in ["Net revenue", "Average order value", "Active customer"] {
        assert!(dependents.contains(&t.to_owned()), "{dependents:?}");
    }
    let visuals = titles(&settled, "visuals", "title");
    assert!(
        visuals.contains(&"Net revenue by region".to_owned()),
        "{visuals:?}"
    );
    assert!(
        visuals.contains(&"Active customers".to_owned()),
        "{visuals:?}"
    );
    assert_eq!(settled["hidden_visuals"], 0);

    let aov = impact("Average order value").await;
    assert_eq!(titles(&aov, "dependents", "title"), Vec::<String>::new());
    let visuals = titles(&aov, "visuals", "title");
    assert!(
        visuals.contains(&"Avg order value".to_owned()),
        "{visuals:?}"
    );
    assert!(
        !visuals.contains(&"Active customers".to_owned()),
        "{visuals:?}"
    );
}
