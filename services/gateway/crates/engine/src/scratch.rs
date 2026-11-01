//! A throwaway in-memory database for checking business definitions against example rows
//! (docs/adr/0006). Nothing outside it is reachable: no files, no extensions, no attachments.

use duckdb::types::Value;
use duckdb::{Config, Connection, params_from_iter};
use lattice_connectors::ident;
use lattice_planner::ParamValue;

use crate::EngineError;
use crate::project::to_value;

/// Example rows for one table.
#[derive(Debug, Clone)]
pub struct FixtureTable {
    pub name: String,
    /// (name, DuckDB type).
    pub columns: Vec<(String, String)>,
    /// One value per column; `None` is NULL.
    pub rows: Vec<Vec<Option<ParamValue>>>,
}

/// Loads `tables` into a fresh database and returns the first column of the first row of
/// `sql`, as a number (NULL, or no rows, is `None`).
pub async fn scalar(
    tables: Vec<FixtureTable>,
    sql: String,
    params: Vec<ParamValue>,
) -> Result<Option<f64>, EngineError> {
    tokio::task::spawn_blocking(move || {
        let config = Config::default()
            .enable_autoload_extension(false)?
            .max_memory("256MB")?
            .threads(1)?;
        let conn = Connection::open_in_memory_with_flags(config)?;
        conn.execute_batch("SET enable_external_access = false; SET lock_configuration = true;")?;
        for t in &tables {
            let columns: Vec<String> = t
                .columns
                .iter()
                .map(|(name, ty)| format!("{} {ty}", ident(name)))
                .collect();
            conn.execute_batch(&format!(
                "CREATE TABLE {} ({})",
                ident(&t.name),
                columns.join(", ")
            ))?;
            let slots: Vec<String> = t
                .columns
                .iter()
                .map(|(_, ty)| format!("CAST(? AS {ty})"))
                .collect();
            let mut insert = conn.prepare(&format!(
                "INSERT INTO {} VALUES ({})",
                ident(&t.name),
                slots.join(", ")
            ))?;
            for row in &t.rows {
                let values: Vec<Value> = row
                    .iter()
                    .map(|v| v.as_ref().map_or(Value::Null, to_value))
                    .collect();
                insert.execute(params_from_iter(values.iter()))?;
            }
        }
        let values: Vec<Value> = params.iter().map(to_value).collect();
        let mut stmt = conn.prepare(&format!(
            "SELECT CAST(COLUMNS(*) AS DOUBLE) FROM ({sql}) AS t LIMIT 1"
        ))?;
        let mut rows = stmt.query(params_from_iter(values.iter()))?;
        Ok(match rows.next()? {
            Some(row) => row.get::<_, Option<f64>>(0)?,
            None => None,
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn computes_over_example_rows() {
        let orders = FixtureTable {
            name: "orders".into(),
            columns: vec![
                ("amount".into(), "DOUBLE".into()),
                ("status".into(), "VARCHAR".into()),
            ],
            rows: vec![
                vec![
                    Some(ParamValue::Double(100.0)),
                    Some(ParamValue::Text("settled".into())),
                ],
                vec![Some(ParamValue::Double(50.0)), None],
            ],
        };
        let sum = scalar(
            vec![orders.clone()],
            "SELECT sum(amount) FILTER (WHERE status = ?) AS v FROM orders".into(),
            vec![ParamValue::Text("settled".into())],
        )
        .await
        .unwrap();
        assert_eq!(sum, Some(100.0));
        let none = scalar(vec![orders], "SELECT NULL AS v".into(), vec![])
            .await
            .unwrap();
        assert_eq!(none, None);
        // The scratch database can't read files.
        let err = scalar(
            vec![],
            "SELECT * FROM read_csv('/etc/hosts')".into(),
            vec![],
        )
        .await;
        assert!(err.is_err());
    }
}
