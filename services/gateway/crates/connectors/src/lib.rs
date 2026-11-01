//! Data-source connectors.
//!
//! DuckDB is the federation engine (docs/adr/0004), so a connector doesn't scan data itself:
//! it validates a source's configuration and produces the DuckDB statements that make the
//! source queryable inside a project database — an attach for live sources, a load for
//! extracts. The engine runs those statements; this crate never touches I/O.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SourceKind {
    File,
    Sqlite,
    Postgres,
    Mysql,
    /// Generated demo dataset.
    Demo,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            SourceKind::File => "file",
            SourceKind::Sqlite => "sqlite",
            SourceKind::Postgres => "postgres",
            SourceKind::Mysql => "mysql",
            SourceKind::Demo => "demo",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "file" => SourceKind::File,
            "sqlite" => SourceKind::Sqlite,
            "postgres" => SourceKind::Postgres,
            "mysql" => SourceKind::Mysql,
            "demo" => SourceKind::Demo,
            _ => return None,
        })
    }

    /// Live sources are attached and scanned in place; the rest are loaded into native tables.
    pub const fn mode(self) -> TableMode {
        match self {
            SourceKind::Sqlite | SourceKind::Postgres | SourceKind::Mysql => TableMode::Live,
            SourceKind::File | SourceKind::Demo => TableMode::Extract,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TableMode {
    Live,
    Extract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FileFormat {
    Csv,
    Tsv,
    Json,
    Ndjson,
    Parquet,
    Xlsx,
}

/// What an uploaded file becomes, decided by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadKind {
    File(FileFormat),
    Sqlite,
}

impl UploadKind {
    pub fn from_file_name(name: &str) -> Option<Self> {
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "csv" => UploadKind::File(FileFormat::Csv),
            "tsv" | "tab" => UploadKind::File(FileFormat::Tsv),
            "json" => UploadKind::File(FileFormat::Json),
            "ndjson" | "jsonl" => UploadKind::File(FileFormat::Ndjson),
            "parquet" => UploadKind::File(FileFormat::Parquet),
            "xlsx" => UploadKind::File(FileFormat::Xlsx),
            "sqlite" | "sqlite3" | "db" => UploadKind::Sqlite,
            _ => return None,
        })
    }
}

/// Non-secret connection settings for a database source. The password is stored separately,
/// encrypted (see `lattice_security::SecretBox`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// Restrict introspection to one schema (Postgres). Defaults to every non-system schema.
    #[serde(default)]
    pub schema: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConnectorError {
    #[error("{0}")]
    Invalid(String),
    #[error("that database is Lattice's own metadata store and can't be added as a source")]
    MetadataDatabase,
}

impl DatabaseConfig {
    pub fn validate(&self) -> Result<(), ConnectorError> {
        let invalid = |m: &str| Err(ConnectorError::Invalid(m.to_owned()));
        let ok_text = |s: &str, max: usize| {
            !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
        };
        if !ok_text(&self.host, 253) || self.host.contains(char::is_whitespace) {
            return invalid("Enter a valid host name.");
        }
        if self.port == 0 {
            return invalid("Enter a valid port.");
        }
        if !ok_text(&self.database, 128) {
            return invalid("Enter a database name.");
        }
        if !ok_text(&self.user, 128) {
            return invalid("Enter a user name.");
        }
        if let Some(schema) = &self.schema
            && !ok_text(schema, 128)
        {
            return invalid("Enter a valid schema name.");
        }
        Ok(())
    }

    /// Refuses the gateway's own metadata database: users could otherwise read sessions and
    /// password hashes through a source. (A general egress allow-list is a production
    /// requirement; see docs/adr/0004.)
    pub fn guard_metadata(&self, metadata: &MetadataDb) -> Result<(), ConnectorError> {
        let same_host = self.host.eq_ignore_ascii_case(&metadata.host)
            || (is_loopback(&self.host) && is_loopback(&metadata.host));
        if same_host && self.port == metadata.port && self.database == metadata.database {
            return Err(ConnectorError::MetadataDatabase);
        }
        Ok(())
    }
}

/// Where the gateway's own Postgres lives (parsed from DATABASE_URL).
#[derive(Debug, Clone)]
pub struct MetadataDb {
    pub host: String,
    pub port: u16,
    pub database: String,
}

fn is_loopback(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    )
}

/// A SQL string literal for DuckDB.
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A quoted SQL identifier for DuckDB.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Statements that attach a live database read-only. Credentials go through a temporary
/// secret, so they never appear in `duckdb_databases().path`.
pub fn attach_database(
    kind: SourceKind,
    alias: &str,
    config: &DatabaseConfig,
    password: &str,
) -> Vec<String> {
    let (secret_type, attach_type) = match kind {
        SourceKind::Mysql => ("mysql", "mysql"),
        _ => ("postgres", "postgres"),
    };
    let secret = format!("sec_{alias}");
    vec![
        format!(
            "CREATE OR REPLACE TEMPORARY SECRET {} (TYPE {secret_type}, HOST {}, PORT {}, DATABASE {}, USER {}, PASSWORD {})",
            ident(&secret),
            literal(&config.host),
            config.port,
            literal(&config.database),
            literal(&config.user),
            literal(password),
        ),
        format!(
            "ATTACH '' AS {} (TYPE {attach_type}, SECRET {}, READ_ONLY)",
            ident(alias),
            ident(&secret)
        ),
    ]
}

pub fn attach_sqlite(alias: &str, path: &str) -> Vec<String> {
    vec![format!(
        "ATTACH {} AS {} (TYPE sqlite, READ_ONLY)",
        literal(path),
        ident(alias)
    )]
}

/// `SELECT` expression reading an uploaded file (`sheet` only for Excel).
pub fn read_file(format: FileFormat, path: &str, sheet: Option<&str>) -> String {
    let p = literal(path);
    match format {
        FileFormat::Csv => format!("read_csv({p}, header = true, auto_detect = true)"),
        FileFormat::Tsv => {
            format!("read_csv({p}, delim = '\\t', header = true, auto_detect = true)")
        }
        FileFormat::Json => format!("read_json_auto({p})"),
        FileFormat::Ndjson => format!("read_json_auto({p}, format = 'newline_delimited')"),
        FileFormat::Parquet => format!("read_parquet({p})"),
        FileFormat::Xlsx => match sheet {
            Some(s) => format!("read_xlsx({p}, sheet = {}, header = true)", literal(s)),
            None => format!("read_xlsx({p}, header = true)"),
        },
    }
}

/// A table name derived from a file or sheet name: lowercase ASCII letters, digits and `_`.
pub fn table_name(raw: &str) -> String {
    let stem = raw.rsplit_once('.').map_or(raw, |(s, _)| s);
    let mut name = String::with_capacity(stem.len());
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
        if name.len() >= 60 {
            break;
        }
    }
    let name = name.trim_end_matches('_');
    match name.chars().next() {
        None => "data".to_owned(),
        Some(c) if c.is_ascii_digit() => format!("t_{name}"),
        Some(_) => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg() -> DatabaseConfig {
        DatabaseConfig {
            host: "db.internal".into(),
            port: 5432,
            database: "acme".into(),
            user: "reader".into(),
            schema: None,
        }
    }

    #[test]
    fn upload_kinds_from_extensions() {
        assert_eq!(
            UploadKind::from_file_name("Sales Q3.CSV"),
            Some(UploadKind::File(FileFormat::Csv))
        );
        assert_eq!(
            UploadKind::from_file_name("targets.xlsx"),
            Some(UploadKind::File(FileFormat::Xlsx))
        );
        assert_eq!(
            UploadKind::from_file_name("events.jsonl"),
            Some(UploadKind::File(FileFormat::Ndjson))
        );
        assert_eq!(
            UploadKind::from_file_name("shop.sqlite"),
            Some(UploadKind::Sqlite)
        );
        assert_eq!(UploadKind::from_file_name("notes.txt"), None);
        assert_eq!(UploadKind::from_file_name("no-extension"), None);
    }

    #[test]
    fn credentials_are_escaped_and_never_in_the_attach_path() {
        let stmts = attach_database(SourceKind::Postgres, "src_1", &pg(), "p'w\"d");
        assert!(stmts[0].contains("PASSWORD 'p''w\"d'"), "{}", stmts[0]);
        assert_eq!(
            stmts[1],
            "ATTACH '' AS \"src_1\" (TYPE postgres, SECRET \"sec_src_1\", READ_ONLY)"
        );
        let mysql = attach_database(SourceKind::Mysql, "src_2", &pg(), "x");
        assert!(mysql[0].contains("TYPE mysql") && mysql[1].contains("TYPE mysql"));
    }

    #[test]
    fn file_readers() {
        assert_eq!(
            read_file(FileFormat::Parquet, "/d/a.parquet", None),
            "read_parquet('/d/a.parquet')"
        );
        assert_eq!(
            read_file(FileFormat::Xlsx, "/d/it's.xlsx", Some("Q3 '26")),
            "read_xlsx('/d/it''s.xlsx', sheet = 'Q3 ''26', header = true)"
        );
    }

    #[test]
    fn table_names() {
        assert_eq!(table_name("FY26_targets.xlsx"), "fy26_targets");
        assert_eq!(
            table_name("Store Regions (final).csv"),
            "store_regions_final"
        );
        assert_eq!(table_name("2026 plan.csv"), "t_2026_plan");
        assert_eq!(table_name("...csv"), "data");
    }

    #[test]
    fn validation() {
        assert!(pg().validate().is_ok());
        assert!(
            DatabaseConfig {
                host: "bad host".into(),
                ..pg()
            }
            .validate()
            .is_err()
        );
        assert!(DatabaseConfig { port: 0, ..pg() }.validate().is_err());
        assert!(
            DatabaseConfig {
                user: " ".into(),
                ..pg()
            }
            .validate()
            .is_err()
        );
        assert!(
            DatabaseConfig {
                database: "a\0b".into(),
                ..pg()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn metadata_database_is_refused() {
        let meta = MetadataDb {
            host: "postgres".into(),
            port: 5432,
            database: "lattice".into(),
        };
        let same = DatabaseConfig {
            host: "POSTGRES".into(),
            database: "lattice".into(),
            ..pg()
        };
        assert_eq!(
            same.guard_metadata(&meta),
            Err(ConnectorError::MetadataDatabase)
        );
        let other_db = DatabaseConfig {
            host: "postgres".into(),
            database: "acme_ops".into(),
            ..pg()
        };
        assert!(other_db.guard_metadata(&meta).is_ok());
        let local_meta = MetadataDb {
            host: "localhost".into(),
            port: 5433,
            database: "lattice".into(),
        };
        let loopback = DatabaseConfig {
            host: "127.0.0.1".into(),
            port: 5433,
            database: "lattice".into(),
            ..pg()
        };
        assert_eq!(
            loopback.guard_metadata(&local_meta),
            Err(ConnectorError::MetadataDatabase)
        );
    }
}
