//! Snapshot tests of the compiler (spec §11): every query shape's SQL and bound parameters.

use lattice_planner::{ColumnInfo, PlanError, TableQuery, TableSchema, compile};
use serde_json::json;

fn orders() -> TableSchema {
    let cols = [
        ("order_date", "DATE"),
        ("created_at", "TIMESTAMP WITH TIME ZONE"),
        ("region", "VARCHAR"),
        ("category", "VARCHAR"),
        ("store", "INTEGER"),
        ("customer", "BIGINT"),
        ("amount", "DECIMAL(12,2)"),
        ("is_online", "BOOLEAN"),
        ("we\"ird name", "VARCHAR"),
    ];
    TableSchema {
        reference: "\"project\".\"s_demo\".\"orders\"".into(),
        columns: cols
            .iter()
            .map(|(n, t)| ColumnInfo {
                name: (*n).into(),
                data_type: (*t).into(),
            })
            .collect(),
    }
}

fn query(spec: serde_json::Value) -> TableQuery {
    let mut v = spec;
    v["table_id"] = json!("01930000-0000-7000-8000-000000000000");
    serde_json::from_value(v).expect("valid TableQuery JSON")
}

fn render(spec: serde_json::Value) -> String {
    let c = compile(&query(spec), &orders()).expect("compiles");
    let cols: Vec<String> = c
        .columns
        .iter()
        .map(|c| format!("{}:{:?}", c.name, c.role))
        .collect();
    format!(
        "{}\n-- params: {:?}\n-- columns: {}\n-- row_limit: {}",
        c.sql,
        c.params,
        cols.join(", "),
        c.row_limit
    )
}

fn error(spec: serde_json::Value) -> PlanError {
    compile(&query(spec), &orders()).expect_err("must be rejected")
}

#[test]
fn sum_by_category_top_10() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "category" }],
        "measures": [{ "column": "amount", "agg": "sum" }],
        "sort": [{ "by": "sum_amount", "dir": "desc" }],
        "limit": 10
    })));
}

#[test]
fn monthly_buckets_use_integer_date_parts() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "order_date", "bucket": "month" }, { "column": "region" }],
        "measures": [{ "agg": "count" }, { "column": "amount", "agg": "sum" }]
    })));
}

#[test]
fn quarter_and_year_buckets() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "created_at", "bucket": "quarter" }, { "column": "order_date", "bucket": "year" }],
        "measures": [{ "column": "amount", "agg": "avg" }]
    })));
}

#[test]
fn week_and_day_buckets_stay_single_level() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "created_at", "bucket": "day" }, { "column": "order_date", "bucket": "week" }],
        "measures": [{ "column": "customer", "agg": "count_distinct" }, { "column": "customer", "agg": "approx_count_distinct" }]
    })));
}

#[test]
fn filters_bind_every_value() {
    insta::assert_snapshot!(render(json!({
        "measures": [{ "agg": "count" }],
        "filters": [
            { "column": "order_date", "op": "between", "value": ["2026-01-01", "2026-03-31"] },
            { "column": "region", "op": "in", "value": ["EMEA", "APAC"] },
            { "column": "store", "op": "eq", "value": 42 },
            { "column": "amount", "op": "gte", "value": 9.99 },
            { "column": "is_online", "op": "eq", "value": true },
            { "column": "category", "op": "contains", "value": "kitchen" },
            { "column": "customer", "op": "is_not_null" }
        ]
    })));
}

#[test]
fn raw_rows_page_with_offset() {
    insta::assert_snapshot!(render(json!({
        "filters": [{ "column": "region", "op": "neq", "value": "AMER" }],
        "sort": [{ "by": "amount", "dir": "desc" }],
        "limit": 50,
        "offset": 100
    })));
}

#[test]
fn raw_points_project_columns_and_allow_a_million_rows() {
    // A scatter plot: three columns, deduplicated, up to MAX_POINTS rows.
    insta::assert_snapshot!(render(json!({
        "columns": ["order_date", "amount", "region", "amount"],
        "filters": [{ "column": "store", "op": "lt", "value": 10 }],
        "limit": 5000000
    })));
}

#[test]
fn wide_raw_queries_keep_the_row_cap() {
    let five = json!({ "columns": ["order_date", "region", "category", "store", "amount"], "limit": 5000000 });
    let c = compile(&query(five), &orders()).expect("compiles");
    assert_eq!(c.row_limit, lattice_planner::MAX_ROWS);
}

#[test]
fn numeric_bins_for_histograms() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "amount", "bin": 25.0 }],
        "measures": [{ "agg": "count" }]
    })));
}

#[test]
fn hostile_input_stays_inside_quotes_and_parameters() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "we\"ird name" }],
        "measures": [{ "agg": "count" }],
        "filters": [{ "column": "region", "op": "eq", "value": "x'; DROP TABLE orders; --" }]
    })));
}

#[test]
fn limits_are_clamped_and_duplicate_names_get_suffixes() {
    insta::assert_snapshot!(render(json!({
        "dimensions": [{ "column": "region" }],
        "measures": [{ "column": "amount", "agg": "sum" }, { "column": "amount", "agg": "sum" }],
        "limit": 5000000
    })));
}

#[test]
fn rejects_invalid_queries() {
    assert_eq!(
        error(json!({ "dimensions": [{ "column": "nope" }] })),
        PlanError::UnknownColumn("nope".into())
    );
    assert!(matches!(
        error(json!({ "measures": [{ "column": "region", "agg": "sum" }] })),
        PlanError::NotNumeric { .. }
    ));
    assert!(matches!(
        error(json!({ "dimensions": [{ "column": "region", "bucket": "month" }] })),
        PlanError::NotTemporal { .. }
    ));
    assert_eq!(
        error(json!({ "measures": [{ "agg": "sum" }] })),
        PlanError::MissingColumn("sum")
    );
    assert!(matches!(
        error(
            json!({ "measures": [{ "agg": "count" }], "filters": [{ "column": "store", "op": "between", "value": [1] }] })
        ),
        PlanError::InvalidFilter { .. }
    ));
    assert!(matches!(
        error(
            json!({ "measures": [{ "agg": "count" }], "filters": [{ "column": "store", "op": "eq", "value": "seven" }] })
        ),
        PlanError::InvalidFilter { .. }
    ));
    assert_eq!(
        error(json!({ "measures": [{ "agg": "count" }], "sort": [{ "by": "amount" }] })),
        PlanError::UnknownSort("amount".into())
    );
    assert!(matches!(
        error(
            json!({ "measures": [{ "agg": "count" }], "filters": [{ "column": "region", "op": "in", "value": [] }] })
        ),
        PlanError::InvalidFilter { .. }
    ));
    assert_eq!(
        error(json!({ "columns": ["amount", "nope"] })),
        PlanError::UnknownColumn("nope".into())
    );
    assert!(matches!(
        error(
            json!({ "dimensions": [{ "column": "region", "bin": 10.0 }], "measures": [{ "agg": "count" }] })
        ),
        PlanError::NotNumeric { agg: "bin", .. }
    ));
    for width in [0.0, -5.0, 1e300] {
        assert!(matches!(
            error(
                json!({ "dimensions": [{ "column": "amount", "bin": width }], "measures": [{ "agg": "count" }] })
            ),
            PlanError::InvalidBin { .. }
        ));
    }
    assert!(matches!(
        error(json!({ "dimensions": [{ "column": "order_date", "bucket": "month", "bin": 1.0 }] })),
        PlanError::InvalidBin { .. }
    ));
}
