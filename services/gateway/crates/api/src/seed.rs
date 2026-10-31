//! Demo data for local development: one workspace that looks like the design's
//! "Acme Retail", with a signed-in-able admin and a few teammates. Idempotent.

use lattice_project::NewProject;
use lattice_security::ProjectRole;
use lattice_security::WorkspaceRole;
use lattice_security::identity::{self, NewAccount};
use lattice_security::password::hash_password;
use sqlx::PgPool;

use crate::error::ApiError;

pub const DEMO_EMAIL: &str = "demo@lattice.dev";
pub const DEMO_PASSWORD: &str = "lattice-demo";

/// Returns false when the demo account already exists (nothing is changed).
pub async fn seed_demo(pool: &PgPool) -> Result<bool, ApiError> {
    if identity::find_password_credentials(pool, DEMO_EMAIL)
        .await?
        .is_some()
    {
        return Ok(false);
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
    }
    Ok(true)
}
