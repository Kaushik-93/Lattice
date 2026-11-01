//! Query execution on DuckDB (docs/adr/0004).
//!
//! Each project has its own database file holding its extracts as native tables, with its live
//! sources attached read-only. A database is opened on first use, hardened (file access limited
//! to the project's directory, configuration locked), and served through a small connection
//! pool. Every query has a queue/execute timing, a cancellation token and a timeout; cancelling
//! interrupts DuckDB mid-query.

pub mod demo;
mod error;
pub mod ipc;
mod project;

pub use error::EngineError;
pub use project::{
    ColumnProfile, IntrospectedColumn, IntrospectedTable, LIVE_PROFILE_ROWS, ProjectDb,
    QueryOutput, Registration,
};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use duckdb::{Config, Connection};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Extensions every project database loads (installed at image build time; see Dockerfile).
pub const EXTENSIONS: [&str; 4] = ["postgres", "mysql", "sqlite", "excel"];

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Projects live in `<data_dir>/projects/<project id>/`.
    pub data_dir: PathBuf,
    pub extension_dir: PathBuf,
    /// DuckDB memory limit per open project, e.g. `"2GB"`.
    pub memory_limit: String,
    /// Threads per open project (None: one per core).
    pub threads: Option<u32>,
    /// Concurrent queries per project.
    pub pool_size: usize,
    pub max_open_projects: usize,
    pub query_timeout: Duration,
}

impl EngineConfig {
    pub fn project_dir(&self, project_id: Uuid) -> PathBuf {
        self.data_dir.join("projects").join(project_id.to_string())
    }
}

/// Owns the open project databases.
pub struct Engine {
    config: Arc<EngineConfig>,
    open: Mutex<Open>,
}

#[derive(Default)]
struct Open {
    live: Vec<(Uuid, Arc<ProjectDb>, Instant)>,
    /// Invalidated databases still referenced by in-flight work; a file may only be open once.
    closing: Vec<(Uuid, Arc<ProjectDb>)>,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config: Arc::new(config),
            open: Mutex::new(Open::default()),
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Downloads any missing extensions into the extension directory. A no-op when they are
    /// already installed, as in the container image.
    pub async fn install_extensions(&self) -> Result<(), EngineError> {
        let dir = self.config.extension_dir.clone();
        tokio::task::spawn_blocking(move || install_extensions(&dir)).await?
    }

    /// The project's database if it is already open.
    pub async fn get(&self, project_id: Uuid) -> Option<Arc<ProjectDb>> {
        let mut open = self.open.lock().await;
        let entry = open.live.iter_mut().find(|(id, _, _)| *id == project_id)?;
        entry.2 = Instant::now();
        Some(entry.1.clone())
    }

    /// Opens the project's database (or returns it if open), attaching `registrations`.
    pub async fn open(
        &self,
        project_id: Uuid,
        registrations: Vec<Registration>,
    ) -> Result<Arc<ProjectDb>, EngineError> {
        let mut open = self.open.lock().await;
        if let Some(entry) = open.live.iter_mut().find(|(id, _, _)| *id == project_id) {
            entry.2 = Instant::now();
            return Ok(entry.1.clone());
        }
        wait_until_closed(&mut open, project_id).await?;
        if open.live.len() >= self.config.max_open_projects
            && let Some(lru) = open
                .live
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(i, _)| i)
        {
            let (id, db, _) = open.live.remove(lru);
            open.closing.push((id, db));
        }
        let config = self.config.clone();
        let db = tokio::task::spawn_blocking(move || {
            ProjectDb::open(project_id, &config, &registrations)
        })
        .await??;
        let db = Arc::new(db);
        open.live.push((project_id, db.clone(), Instant::now()));
        Ok(db)
    }

    /// Closes a project's database so the next `open` re-attaches its sources (DuckDB can't
    /// attach after the configuration is locked). In-flight queries finish first.
    pub async fn invalidate(&self, project_id: Uuid) {
        let mut open = self.open.lock().await;
        if let Some(i) = open.live.iter().position(|(id, _, _)| *id == project_id) {
            let (id, db, _) = open.live.remove(i);
            open.closing.push((id, db));
        }
    }

    /// Attaches a source on a throwaway in-memory database and introspects it: checks
    /// credentials and reachability before anything is saved.
    pub async fn probe(
        &self,
        registration: Registration,
        schema: Option<String>,
        timeout: Duration,
    ) -> Result<Vec<IntrospectedTable>, EngineError> {
        let ext = self.config.extension_dir.clone();
        let task = tokio::task::spawn_blocking(move || {
            let conn = Connection::open_in_memory_with_flags(
                Config::default()
                    .with("extension_directory", path_str(&ext)?)?
                    .with("autoinstall_known_extensions", "false")?,
            )?;
            for e in EXTENSIONS {
                conn.execute_batch(&format!("LOAD {e}"))?;
            }
            for statement in &registration.statements {
                conn.execute_batch(statement)?;
            }
            project::introspect_conn(&conn, &registration.alias, schema.as_deref())
        });
        match tokio::time::timeout(timeout, task).await {
            Ok(joined) => joined?,
            Err(_) => Err(EngineError::Timeout(timeout)),
        }
    }

    pub async fn open_projects(&self) -> usize {
        self.open.lock().await.live.len()
    }
}

async fn wait_until_closed(open: &mut Open, project_id: Uuid) -> Result<(), EngineError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        open.closing.retain(|(_, db)| Arc::strong_count(db) > 1);
        if !open.closing.iter().any(|(id, _)| *id == project_id) {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(EngineError::Busy);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn install_extensions(dir: &Path) -> Result<(), EngineError> {
    std::fs::create_dir_all(dir)?;
    let conn = Connection::open_in_memory_with_flags(
        Config::default().with("extension_directory", path_str(dir)?)?,
    )?;
    for ext in EXTENSIONS {
        conn.execute_batch(&format!("INSTALL {ext}"))?;
    }
    Ok(())
}

pub(crate) fn path_str(path: &Path) -> Result<&str, EngineError> {
    path.to_str()
        .ok_or_else(|| EngineError::Path(path.display().to_string()))
}
