//! The knowledge graph end to end: the draft → pending → approved lifecycle with owners,
//! links and backlinks resolved by title, scoping, and the review queue.

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn note(h: &Harness, cookie: &str, body: Value) -> Value {
    let reply = json_call(
        &h.app,
        "POST",
        "/api/knowledge/notes",
        Some(cookie),
        Some(body),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

async fn act(h: &Harness, cookie: &str, id: &Value, action: &str, body: Option<Value>) -> Reply {
    json_call(
        &h.app,
        "POST",
        &format!("/api/knowledge/notes/{}/{action}", id.as_str().unwrap()),
        Some(cookie),
        body,
    )
    .await
}

async fn graph(h: &Harness, cookie: &str) -> Value {
    json_call(&h.app, "GET", "/api/knowledge/graph", Some(cookie), None)
        .await
        .body
}

/// Puts `email` in the workspace of `project`'s owner, optionally with a project role.
async fn join(pool: &PgPool, project: Uuid, email: &str, role: Option<&str>) {
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
    if let Some(role) = role {
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
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn notes_move_from_draft_to_approved_with_their_owner(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let ana = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &ana).await;
    let priya = signup(&h.app, "priya@acme.example").await;
    join(&pool, project, "priya@acme.example", Some("editor")).await;

    // Priya (a workspace member) drafts a shared metric; she owns it.
    let created = note(
        &h,
        &priya,
        json!({
            "kind": "metric", "title": "Net revenue", "owner_team": "Finance",
            "body": "Revenue from [[Order]]s that are settled.",
            "definition": { "formula": "SUM(order.amount)" }
        }),
    )
    .await;
    let id = created["id"].clone();
    assert_eq!(created["state"], "draft");
    assert_eq!(created["owner"]["team"], "Finance");
    assert_eq!(
        created["can"]["review"], true,
        "owners review their own notes"
    );

    // Titles are unique per scope, ignoring case and spacing.
    let dup = json_call(
        &h.app,
        "POST",
        "/api/knowledge/notes",
        Some(&ana),
        Some(json!({ "kind": "metric", "title": "net  REVENUE" })),
    )
    .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let submitted = act(&h, &priya, &id, "submit", None).await;
    assert_eq!(submitted.body["state"], "pending");
    // Priya hands ownership to Ana (the workspace admin); no longer the owner, and not an
    // admin, Priya can't approve it.
    let ana_id = sqlx::query_scalar!("SELECT id FROM users WHERE email = 'ana@acme.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let handed = json_call(
        &h.app,
        "PUT",
        &format!("/api/knowledge/notes/{}/owner", id.as_str().unwrap()),
        Some(&priya),
        Some(json!({ "owner_id": ana_id, "team": "Finance" })),
    )
    .await;
    assert_eq!(handed.status, StatusCode::OK, "{:?}", handed.body);
    assert_eq!(
        act(&h, &priya, &id, "approve", None).await.status,
        StatusCode::FORBIDDEN
    );

    let approved = act(&h, &ana, &id, "approve", None).await;
    assert_eq!(approved.body["state"], "approved");
    assert_eq!(approved.body["live"]["version"], 1);

    // A change to approved knowledge waits for review while version 1 keeps serving.
    let path = format!("/api/knowledge/notes/{}/draft", id.as_str().unwrap());
    let edit = json_call(
        &h.app,
        "PUT",
        &path,
        Some(&priya),
        Some(json!({
            "title": "Net revenue", "body": "Settled orders minus [[Refund]]s.",
            "change_note": "Refunds were missing"
        })),
    )
    .await;
    assert_eq!(edit.body["state"], "change_draft");
    assert_eq!(edit.body["live"]["version"], 1);
    assert_eq!(edit.body["open"]["version"], 2);
    act(&h, &priya, &id, "submit", None).await;
    assert_eq!(
        act(&h, &ana, &id, "reject", Some(json!({ "reason": "  " })))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let rejected = act(
        &h,
        &ana,
        &id,
        "reject",
        Some(json!({ "reason": "Refunds are already excluded by status." })),
    )
    .await;
    assert_eq!(rejected.body["state"], "approved");
    assert_eq!(rejected.body["history"][0]["status"], "rejected");
    assert_eq!(
        rejected.body["history"][0]["review_reason"],
        "Refunds are already excluded by status."
    );

    // Revert starts a draft from an older version; approving it supersedes the live one.
    act(&h, &priya, &id, "revert", Some(json!({ "version": 1 }))).await;
    act(&h, &priya, &id, "submit", None).await;
    let back = act(&h, &ana, &id, "approve", None).await;
    assert_eq!(back.body["live"]["version"], 3);
    let statuses: Vec<&str> = back.body["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["approved", "rejected", "superseded"]);

    let gone = act(
        &h,
        &ana,
        &id,
        "deprecate",
        Some(json!({ "deprecated": true })),
    )
    .await;
    assert_eq!(gone.body["state"], "deprecated");
    let audit = sqlx::query_scalar!("SELECT count(*) FROM audit_log WHERE action LIKE 'note.%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(audit.unwrap() >= 8);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn links_resolve_by_title_like_obsidian(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let ana = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &ana).await;

    let revenue = note(
        &h,
        &ana,
        json!({ "kind": "metric", "title": "Net revenue",
        "body": "From [[order]]s, see [[Fiscal calendar|the calendar]] and [[Nowhere yet]]." }),
    )
    .await;
    let g = graph(&h, &ana).await;
    let ghosts: Vec<&str> = g["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["title"].as_str().unwrap())
        .collect();
    assert_eq!(ghosts, ["order", "Fiscal calendar", "Nowhere yet"]);

    // Writing the note a link points to resolves it, whatever the case.
    let order = note(&h, &ana, json!({ "kind": "entity", "title": "Order" })).await;
    let g = graph(&h, &ana).await;
    assert_eq!(
        g["edges"],
        json!([{ "from": revenue["id"], "to": order["id"] }])
    );
    let detail = json_call(
        &h.app,
        "GET",
        &format!("/api/knowledge/notes/{}", order["id"].as_str().unwrap()),
        Some(&ana),
        None,
    )
    .await;
    assert_eq!(detail.body["backlinks"][0]["title"], "Net revenue");

    // A project's own note wins over a shared one with the same title, for that project's links.
    let local = note(
        &h,
        &ana,
        json!({ "kind": "entity", "title": "Order", "project_id": project }),
    )
    .await;
    let local_link = note(
        &h,
        &ana,
        json!({ "kind": "fact", "title": "Local fact", "project_id": project,
        "body": "Counts [[Order]]s." }),
    )
    .await;
    let g = json_call(
        &h.app,
        "GET",
        &format!("/api/knowledge/graph?project={project}"),
        Some(&ana),
        None,
    )
    .await
    .body;
    assert!(
        g["edges"]
            .as_array()
            .unwrap()
            .contains(&json!({ "from": local_link["id"], "to": local["id"] }))
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn project_notes_stay_in_their_project(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let ana = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &ana).await;
    let vic = signup(&h.app, "vic@acme.example").await;
    join(&pool, project, "vic@acme.example", None).await;

    let local = note(
        &h,
        &ana,
        json!({ "kind": "fact", "title": "Board pack", "project_id": project }),
    )
    .await;
    note(
        &h,
        &ana,
        json!({ "kind": "term", "title": "Sales", "submit": true }),
    )
    .await;
    // Vic is in the workspace but not the project: shared notes only.
    let titles: Vec<String> = graph(&h, &vic).await["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["title"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(titles, ["Sales"]);
    let hidden = json_call(
        &h.app,
        "GET",
        &format!("/api/knowledge/notes/{}", local["id"].as_str().unwrap()),
        Some(&vic),
        None,
    )
    .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    // As a project viewer he sees it but may not draft the project's notes.
    sqlx::query!("INSERT INTO project_members (project_id, user_id, role) SELECT $1, id, 'viewer' FROM users WHERE email = 'vic@acme.example'", project)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(graph(&h, &vic).await["nodes"].as_array().unwrap().len(), 2);
    let refused = json_call(
        &h.app,
        "POST",
        "/api/knowledge/notes",
        Some(&vic),
        Some(json!({ "kind": "fact", "title": "Mine", "project_id": project })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    // The review queue: Ana (owner) sees the pending term; Vic sees nothing to review.
    let queue = json_call(&h.app, "GET", "/api/knowledge/queue", Some(&ana), None)
        .await
        .body;
    let pending: Vec<&str> = queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["note"]["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        pending,
        ["Sales", "Board pack"],
        "pending first, then her drafts"
    );
    let theirs = json_call(&h.app, "GET", "/api/knowledge/queue", Some(&vic), None)
        .await
        .body;
    assert_eq!(theirs["items"], json!([]));
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn the_demo_seeds_a_graph_in_every_state(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let demo = lattice_api::seed::seed_demo(&pool).await.unwrap().unwrap();
    lattice_api::seed::seed_demo_knowledge(&pool, &demo)
        .await
        .unwrap();
    let cookie = json_call(&h.app, "POST", "/api/auth/login", None,
        Some(json!({ "email": lattice_api::seed::DEMO_EMAIL, "password": lattice_api::seed::DEMO_PASSWORD }))).await.cookie.unwrap();
    let g = graph(&h, &cookie).await;
    let state = |title: &str| {
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["title"] == title)
            .unwrap()["state"]
            .clone()
    };
    assert_eq!(g["nodes"].as_array().unwrap().len(), 19);
    assert_eq!(state("Net revenue"), "approved");
    assert_eq!(state("Churned customer"), "pending");
    assert_eq!(state("Active customer"), "change_pending");
    assert_eq!(state("Refund rate"), "draft");
    assert!(g["edges"].as_array().unwrap().len() > 20);
    assert_eq!(
        g["unresolved"].as_array().unwrap()[0]["title"],
        "Revenue Review"
    );
}
