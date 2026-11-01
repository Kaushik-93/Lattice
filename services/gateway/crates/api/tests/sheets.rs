//! Sheets end to end: storage with optimistic concurrency, sheet batches over the data socket
//! (sheet filters, cross-filtering, KPI comparisons), and share links for each audience.

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use common::*;
use futures_util::SinkExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

async fn orders_table(h: &Harness, cookie: &str, project: Uuid) -> Uuid {
    let reply = upload(&h.app, cookie, project, "orders.csv", ORDERS_CSV.as_bytes()).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body["tables"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

async fn create_sheet(h: &Harness, cookie: &str, project: Uuid, name: &str) -> Value {
    let reply = json_call(
        &h.app,
        "POST",
        &format!("/api/projects/{project}/sheets"),
        Some(cookie),
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

async fn save(h: &Harness, cookie: &str, project: Uuid, sheet: &Value, body: Value) -> Reply {
    json_call(
        &h.app,
        "PUT",
        &format!(
            "/api/projects/{project}/sheets/{}",
            sheet["id"].as_str().unwrap()
        ),
        Some(cookie),
        Some(body),
    )
    .await
}

/// Puts `email`'s account in the project's workspace with `role` (as an admin's invite would).
async fn join_project(pool: &PgPool, project: Uuid, email: &str, role: &str) {
    let user = sqlx::query_scalar!("SELECT id FROM users WHERE email = $1", email)
        .fetch_one(pool)
        .await
        .unwrap();
    let workspace = sqlx::query_scalar!("SELECT workspace_id FROM projects WHERE id = $1", project)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')
         ON CONFLICT DO NOTHING",
        workspace,
        user
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!(
        "UPDATE sessions SET workspace_id = $1 WHERE user_id = $2",
        workspace,
        user
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3)",
        project,
        user,
        role
    )
    .execute(pool)
    .await
    .unwrap();
}

fn kpi(table: Uuid) -> Value {
    json!({
        "kind": "kpi", "title": "Net revenue", "table_id": table,
        "measures": [{ "column": "amount", "agg": "sum" }],
        "options": { "compare": "order_date" }
    })
}

fn by(table: Uuid, kind: &str, column: &str) -> Value {
    json!({
        "kind": kind, "title": format!("By {column}"), "table_id": table,
        "x": { "column": column, "bucket": null },
        "measures": [{ "column": "amount", "agg": "sum" }]
    })
}

fn layout(x: u16) -> Value {
    json!({ "x": x, "y": 0, "w": 4, "h": 3 })
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn sheets_save_with_optimistic_concurrency(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let owner = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &owner).await;
    let table = orders_table(&h, &owner, project).await;

    let first = create_sheet(&h, &owner, project, "Revenue Review").await;
    let second = create_sheet(&h, &owner, project, "Retention").await;
    assert_eq!(
        (first["version"].clone(), first["position"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(second["position"], 2);

    let v1 = Uuid::new_v4();
    let body = |version: i64, x: u16| {
        json!({
            "version": version, "name": "Revenue Review", "filters": [],
            "visuals": [{ "id": v1, "spec": kpi(table), "layout": layout(x) }]
        })
    };
    let saved = save(&h, &owner, project, &first, body(1, 0)).await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    assert_eq!(saved.body["version"], 2);
    // A second editor still on version 1 is refused instead of overwriting.
    let stale = save(&h, &owner, project, &first, body(1, 4)).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    // Off the 12-column grid.
    assert_eq!(
        save(&h, &owner, project, &first, body(2, 10)).await.status,
        StatusCode::BAD_REQUEST
    );
    // A visual id already used on another sheet can't be taken over.
    let stolen = save(
        &h,
        &owner,
        project,
        &second,
        json!({
            "version": 1, "name": "Retention", "filters": [],
            "visuals": [{ "id": v1, "spec": kpi(table), "layout": layout(0) }]
        }),
    )
    .await;
    assert_eq!(stolen.status, StatusCode::BAD_REQUEST, "{:?}", stolen.body);

    let got = json_call(
        &h.app,
        "GET",
        &format!(
            "/api/projects/{project}/sheets/{}",
            first["id"].as_str().unwrap()
        ),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(got.body["visuals"][0]["id"], json!(v1));
    assert_eq!(
        got.body["visuals"][0]["spec"]["options"]["compare"],
        "order_date"
    );
    assert!(
        got.body["visuals"][0]["spec"].get("x").is_none(),
        "optional fields stay omitted"
    );

    // Reorder, then list.
    let reorder = json_call(
        &h.app,
        "POST",
        &format!("/api/projects/{project}/sheets/order"),
        Some(&owner),
        Some(json!({ "ids": [second["id"], first["id"]] })),
    )
    .await;
    assert_eq!(reorder.status, StatusCode::NO_CONTENT);
    let list = json_call(
        &h.app,
        "GET",
        &format!("/api/projects/{project}/sheets"),
        Some(&owner),
        None,
    )
    .await;
    let names: Vec<&str> = list.body["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Retention", "Revenue Review"]);
    assert_eq!(list.body["sheets"][1]["visual_count"], 1);

    // Viewers read sheets but can't change them.
    signup(&h.app, "vic@acme.example").await;
    let viewer = json_call(
        &h.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": "vic@acme.example", "password": "correct-horse-battery" })),
    )
    .await
    .cookie
    .unwrap();
    join_project(&pool, project, "vic@acme.example", "viewer").await;
    let path = format!(
        "/api/projects/{project}/sheets/{}",
        first["id"].as_str().unwrap()
    );
    assert_eq!(
        json_call(&h.app, "GET", &path, Some(&viewer), None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        json_call(&h.app, "DELETE", &path, Some(&viewer), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(&h.app, "DELETE", &path, Some(&owner), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        json_call(&h.app, "GET", &path, Some(&owner), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn a_sheet_batch_applies_filters_selections_and_comparisons(pool: PgPool) {
    let h = harness(pool).await;
    let cookie = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &cookie).await;
    let table = orders_table(&h, &cookie, project).await;
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &cookie).await;

    let (k, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let batch = |batch_id: u32| {
        json!({
            "type": "sheet_batch", "batch_id": batch_id, "project_id": project,
            "context": {
                // February–March; the KPI compares with December–January.
                "filters": [{ "column": "order_date", "op": "between", "value": ["2026-02-01", "2026-03-31"] }],
                // Outdoor picked on the category chart.
                "selections": [{ "visual_id": c, "table_id": table,
                                 "filter": { "column": "category", "op": "eq", "value": "Outdoor" } }]
            },
            "visuals": [
                { "id": k, "spec": kpi(table) },
                { "id": b, "spec": by(table, "bar", "region") },
                { "id": c, "spec": by(table, "column", "category") }
            ]
        })
    };
    ws.send(Message::Text(batch(1).to_string().into()))
        .await
        .unwrap();
    let mut rows = std::collections::BTreeMap::new();
    let mut fingerprints = std::collections::BTreeMap::new();
    for event in until_done(&mut ws, 1).await {
        match event {
            Event::Result(header, n) => {
                fingerprints.insert(header.query_id.clone(), header.fingerprint.clone());
                rows.insert(header.query_id, n);
            }
            Event::Error(e) => panic!("{e}"),
            Event::Done(_) => {}
        }
    }
    let expected = [
        (k.to_string(), 1),
        (format!("{k}:compare"), 1),
        // Only AMER sold Outdoor in February–March.
        (b.to_string(), 1),
        // The category chart isn't filtered by its own selection.
        (c.to_string(), 2),
    ];
    assert_eq!(rows, expected.into_iter().collect());

    // The same state again: served from cache with identical fingerprints, so the browser can
    // skip re-rendering.
    ws.send(Message::Text(batch(2).to_string().into()))
        .await
        .unwrap();
    for event in until_done(&mut ws, 2).await {
        if let Event::Result(header, _) = event {
            assert!(header.cached);
            assert_eq!(fingerprints[&header.query_id], header.fingerprint);
        }
    }

    // An incomplete spec fails alone; the rest of the batch still answers.
    let msg = json!({
        "type": "sheet_batch", "batch_id": 3, "project_id": project,
        "visuals": [
            { "id": k, "spec": { "kind": "line", "title": "x", "table_id": table } },
            { "id": b, "spec": by(table, "bar", "region") }
        ]
    });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
    let events = until_done(&mut ws, 3).await;
    assert!(events.iter().any(|e| matches!(e, Event::Error(v) if v["query_id"] == json!(k.to_string()) && v["code"] == "invalid_query")));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Result(h, _) if h.query_id == b.to_string()))
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn share_links_respect_their_audience(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let owner = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &owner).await;
    let table = orders_table(&h, &owner, project).await;
    let sheet = create_sheet(&h, &owner, project, "Revenue Review").await;
    let visual = Uuid::new_v4();
    let saved = save(&h, &owner, project, &sheet, json!({
        "version": 1, "name": "Revenue Review",
        "filters": [{ "id": Uuid::new_v4(), "column": "region", "label": "Region", "kind": "values" }],
        "visuals": [{ "id": visual, "spec": by(table, "bar", "category"), "layout": layout(0) }]
    }))
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);

    let sharing_path = format!(
        "/api/projects/{project}/sheets/{}/sharing",
        sheet["id"].as_str().unwrap()
    );
    let share = |audience: &str| json!({ "audience": audience, "expires_in_days": 30, "allow_export": false, "track_views": true });
    let reply = json_call(
        &h.app,
        "PUT",
        &sharing_path,
        Some(&owner),
        Some(share("public")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let token = reply.body["token"].as_str().unwrap().to_owned();
    let link = format!("/api/shared/{token}");

    // Public: no sign-in needed, and the view is counted.
    let view = json_call(&h.app, "GET", &link, None, None).await;
    assert_eq!(view.status, StatusCode::OK, "{:?}", view.body);
    assert_eq!(view.body["sheet"]["visuals"].as_array().unwrap().len(), 1);
    assert_eq!(view.body["project_name"], "Revenue");
    let views = json_call(&h.app, "GET", &sharing_path, Some(&owner), None).await;
    assert_eq!(views.body["view_count"], 1);

    // An anonymous socket may run the sheet's own visual with the sheet's own filter…
    let addr = serve(h.state.clone()).await;
    let mut ws = connect_as(addr, None).await;
    let shared = |batch_id: u32, context: Value, id: Uuid| {
        json!({ "type": "shared_batch", "batch_id": batch_id, "token": token, "context": context,
                "visuals": [{ "id": id }] })
    };
    let region = json!({ "filters": [{ "column": "region", "op": "eq", "value": "EMEA" }] });
    ws.send(Message::Text(shared(1, region, visual).to_string().into()))
        .await
        .unwrap();
    let events = until_done(&mut ws, 1).await;
    // EMEA sold Outdoor and Apparel.
    assert!(
        events.iter().any(|e| matches!(e, Event::Result(_, 2))),
        "events: {}",
        events.len()
    );

    // …but not filter on columns the sheet doesn't offer, nor run anything else.
    let probe = json!({ "filters": [{ "column": "amount", "op": "gt", "value": 5 }] });
    ws.send(Message::Text(shared(2, probe, visual).to_string().into()))
        .await
        .unwrap();
    assert!(
        until_done(&mut ws, 2)
            .await
            .iter()
            .any(|e| matches!(e, Event::Error(v) if v["code"] == "invalid_request"))
    );
    ws.send(Message::Text(
        shared(3, json!({}), Uuid::new_v4()).to_string().into(),
    ))
    .await
    .unwrap();
    assert!(
        until_done(&mut ws, 3)
            .await
            .iter()
            .any(|e| matches!(e, Event::Error(v) if v["code"] == "not_found"))
    );
    let explorer = json!({ "type": "query_batch", "batch_id": 4, "project_id": project,
                           "queries": [{ "id": "q", "query": { "table_id": table } }] });
    ws.send(Message::Text(explorer.to_string().into()))
        .await
        .unwrap();
    let refused = next_json(&mut ws).await;
    assert_eq!(refused["code"], "session_expired");

    // Workspace links need a signed-in member of the workspace.
    let other = signup(&h.app, "otto@elsewhere.example").await;
    json_call(
        &h.app,
        "PUT",
        &sharing_path,
        Some(&owner),
        Some(share("workspace")),
    )
    .await;
    assert_eq!(
        json_call(&h.app, "GET", &link, None, None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        json_call(&h.app, "GET", &link, Some(&other), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(&h.app, "GET", &link, Some(&owner), None)
            .await
            .status,
        StatusCode::OK
    );

    // Invitations are for people already in the workspace.
    let invite = json_call(
        &h.app,
        "POST",
        &format!(
            "/api/projects/{project}/sheets/{}/members",
            sheet["id"].as_str().unwrap()
        ),
        Some(&owner),
        Some(json!({ "email": "otto@elsewhere.example" })),
    )
    .await;
    assert_eq!(invite.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Expired and switched-off links look the same as links that never existed.
    json_call(
        &h.app,
        "PUT",
        &sharing_path,
        Some(&owner),
        Some(share("public")),
    )
    .await;
    sqlx::query!("UPDATE sheets SET link_expires_at = now() - interval '1 day'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        json_call(&h.app, "GET", &link, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    let off = json!({ "audience": "off", "allow_export": false, "track_views": true });
    let reply = json_call(&h.app, "PUT", &sharing_path, Some(&owner), Some(off)).await;
    assert_eq!(reply.body["token"], Value::Null);
    assert_eq!(
        json_call(&h.app, "GET", &link, Some(&owner), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn every_demo_sheet_visual_runs(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let demo = lattice_api::seed::seed_demo(&pool).await.unwrap().unwrap();
    lattice_api::seed::seed_demo_data(&h.state, &demo, 20_000)
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
    let project = demo.revenue_project;
    let list = json_call(
        &h.app,
        "GET",
        &format!("/api/projects/{project}/sheets"),
        Some(&cookie),
        None,
    )
    .await;
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &cookie).await;
    let mut kinds = std::collections::BTreeSet::new();
    for (i, summary) in list.body["sheets"].as_array().unwrap().iter().enumerate() {
        let sheet = json_call(
            &h.app,
            "GET",
            &format!(
                "/api/projects/{project}/sheets/{}",
                summary["id"].as_str().unwrap()
            ),
            Some(&cookie),
            None,
        )
        .await
        .body;
        // Default filter values become the sheet context, as the web app does.
        let filters: Vec<Value> = sheet["filters"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f.get("value").is_some())
            .map(|f| {
                let op = if f["kind"] == "date_range" {
                    "between"
                } else {
                    "in"
                };
                json!({ "column": f["column"], "op": op, "value": f["value"] })
            })
            .collect();
        let visuals: Vec<Value> = sheet["visuals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                kinds.insert(v["spec"]["kind"].as_str().unwrap().to_owned());
                json!({ "id": v["id"], "spec": v["spec"] })
            })
            .collect();
        let batch_id = u32::try_from(i).unwrap() + 1;
        let msg = json!({ "type": "sheet_batch", "batch_id": batch_id, "project_id": project,
                          "context": { "filters": filters }, "visuals": visuals });
        ws.send(Message::Text(msg.to_string().into()))
            .await
            .unwrap();
        for event in until_done(&mut ws, batch_id).await {
            if let Event::Error(e) = event {
                panic!("{}: {e}", summary["name"]);
            }
            if let Event::Result(header, rows) = event {
                assert!(
                    rows > 0,
                    "{}: {} returned no rows",
                    summary["name"],
                    header.query_id
                );
            }
        }
    }
    let expected = [
        "area",
        "bar",
        "donut",
        "heatmap",
        "histogram",
        "kpi",
        "pivot",
        "scatter",
        "slicer",
        "table",
    ];
    assert_eq!(kinds, expected.iter().map(|k| (*k).to_owned()).collect());
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn project_admins_manage_members(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let owner = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &owner).await;
    // Priya has an account in the workspace but no role in the project yet.
    signup(&h.app, "priya@acme.example").await;
    let workspace = sqlx::query_scalar!("SELECT workspace_id FROM projects WHERE id = $1", project)
        .fetch_one(&pool)
        .await
        .unwrap();
    let priya = sqlx::query_scalar!("SELECT id FROM users WHERE email = 'priya@acme.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        workspace,
        priya
    )
    .execute(&pool)
    .await
    .unwrap();
    let ana = sqlx::query_scalar!("SELECT id FROM users WHERE email = 'ana@acme.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let members = format!("/api/projects/{project}/members");

    let added = json_call(
        &h.app,
        "POST",
        &members,
        Some(&owner),
        Some(json!({ "email": "Priya@acme.example", "role": "editor" })),
    )
    .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let roles: Vec<(String, String)> = added.body["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["email"].as_str().unwrap().to_owned(),
                m["role"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        roles.contains(&("priya@acme.example".into(), "editor".into())),
        "{roles:?}"
    );

    let outsider = json_call(
        &h.app,
        "POST",
        &members,
        Some(&owner),
        Some(json!({ "email": "nobody@else.example", "role": "viewer" })),
    )
    .await;
    assert_eq!(outsider.status, StatusCode::UNPROCESSABLE_ENTITY);

    let promote = json_call(
        &h.app,
        "PATCH",
        &format!("{members}/{priya}"),
        Some(&owner),
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(promote.status, StatusCode::OK);
    // The owner always stays an admin and can't be removed.
    let demote_owner = json_call(
        &h.app,
        "PATCH",
        &format!("{members}/{ana}"),
        Some(&owner),
        Some(json!({ "role": "viewer" })),
    )
    .await;
    assert_eq!(demote_owner.status, StatusCode::UNPROCESSABLE_ENTITY);
    let remove_owner = json_call(
        &h.app,
        "DELETE",
        &format!("{members}/{ana}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(remove_owner.status, StatusCode::UNPROCESSABLE_ENTITY);

    let removed = json_call(
        &h.app,
        "DELETE",
        &format!("{members}/{priya}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(removed.body["members"].as_array().unwrap().len(), 1);
    let audit =
        sqlx::query_scalar!("SELECT count(*) FROM audit_log WHERE action LIKE 'project.member.%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audit, Some(3));
}
