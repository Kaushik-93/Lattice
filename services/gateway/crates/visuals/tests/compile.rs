//! Visual specs compile to the table queries the planner expects, with sheet filters,
//! cross-filter selections, drill paths and KPI comparisons applied.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use lattice_planner::{ColumnInfo, TableQuery, TableSchema};
use lattice_visuals::{QueryPart, SheetContext, VisualError, VisualSpec, compile};
use serde_json::{Value, json};
use uuid::Uuid;

const ORDERS: &str = "01930000-0000-7000-8000-000000000001";
const STORES: &str = "01930000-0000-7000-8000-000000000002";
const ME: &str = "01930000-0000-7000-8000-0000000000aa";
const OTHER: &str = "01930000-0000-7000-8000-0000000000bb";

fn orders() -> TableSchema {
    let cols = [
        ("order_date", "DATE"),
        ("region", "VARCHAR"),
        ("category", "VARCHAR"),
        ("store_id", "INTEGER"),
        ("amount", "DECIMAL(12,2)"),
        ("quantity", "INTEGER"),
    ];
    TableSchema {
        reference: "\"project\".\"s\".\"orders\"".into(),
        columns: cols
            .iter()
            .map(|(n, t)| ColumnInfo {
                name: (*n).into(),
                data_type: (*t).into(),
            })
            .collect(),
        fiscal_year_start_month: 1,
    }
}

fn spec(v: Value) -> VisualSpec {
    let mut v = v;
    v["title"] = json!("t");
    if v.get("table_id").is_none() {
        v["table_id"] = json!(ORDERS);
    }
    serde_json::from_value(v).expect("valid spec")
}

fn run(
    spec_json: Value,
    drill: Value,
    ctx: Value,
) -> Result<Vec<(QueryPart, TableQuery)>, VisualError> {
    let ctx: SheetContext = serde_json::from_value(ctx).expect("valid context");
    let drill = serde_json::from_value::<Vec<lattice_planner::Scalar>>(drill).expect("drill");
    compile(
        ME.parse().unwrap(),
        &spec(spec_json),
        &drill,
        None,
        &ctx,
        &orders(),
    )
    .map(|qs| qs.into_iter().map(|q| (q.part, q.query)).collect())
}

fn one(spec_json: Value) -> Value {
    let out = run(spec_json, json!([]), json!({})).expect("compiles");
    assert_eq!(out.len(), 1);
    serde_json::to_value(&out[0].1).unwrap()
}

#[test]
fn ranked_bars_sort_by_value_and_time_series_by_date() {
    let bars = one(json!({
        "kind": "bar",
        "x": { "column": "region", "bucket": null },
        "measures": [{ "column": "amount", "agg": "sum" }]
    }));
    assert_eq!(bars["sort"], json!([{ "by": "sum_amount", "dir": "desc" }]));
    assert_eq!(bars["limit"], 50);

    let line = one(json!({
        "kind": "line",
        "x": { "column": "order_date", "bucket": "month" },
        "series": { "column": "region", "bucket": null },
        "measures": [{ "column": "amount", "agg": "sum" }]
    }));
    assert_eq!(line["dimensions"].as_array().unwrap().len(), 2);
    assert_eq!(
        line["sort"],
        json!([]),
        "planner orders by dimensions by default"
    );
}

#[test]
fn scatter_projects_points_and_caps_at_a_million() {
    let q = one(json!({
        "kind": "scatter",
        "x": { "column": "order_date", "bucket": null },
        "columns": ["amount"],
        "series": { "column": "region", "bucket": null },
        "limit": 5000000
    }));
    assert_eq!(q["columns"], json!(["order_date", "amount", "region"]));
    assert_eq!(q["limit"], 1_000_000);
}

#[test]
fn histograms_and_slicers_count_by_default() {
    let h = one(
        json!({ "kind": "histogram", "x": { "column": "amount", "bucket": null, "bin": 50.0 } }),
    );
    assert_eq!(h["measures"], json!([{ "column": null, "agg": "count" }]));
    let missing = run(
        json!({ "kind": "histogram", "x": { "column": "amount", "bucket": null } }),
        json!([]),
        json!({}),
    );
    assert!(matches!(
        missing,
        Err(VisualError::Missing {
            what: "a bin width",
            ..
        })
    ));
}

#[test]
fn sheet_filters_apply_only_where_the_column_exists() {
    let out = run(
        json!({ "kind": "kpi", "measures": [{ "column": "amount", "agg": "sum" }] }),
        json!([]),
        json!({ "filters": [
            { "column": "region", "op": "eq", "value": "EMEA" },
            { "column": "signed_up_on", "op": "gte", "value": "2025-01-01" }
        ] }),
    )
    .unwrap();
    let filters: Vec<&str> = out[0].1.filters.iter().map(|f| f.column.as_str()).collect();
    assert_eq!(filters, ["region"]);
}

#[test]
fn selections_cross_filter_the_same_table_but_never_their_own_visual() {
    let ctx = json!({ "selections": [
        { "visual_id": OTHER, "table_id": ORDERS, "filter": { "column": "category", "op": "eq", "value": "Outdoor" } },
        { "visual_id": OTHER, "table_id": STORES, "filter": { "column": "region", "op": "eq", "value": "APAC" } },
        { "visual_id": OTHER, "table_id": STORES, "scope": "sheet", "filter": { "column": "store_id", "op": "eq", "value": 7 } },
        { "visual_id": ME, "table_id": ORDERS, "filter": { "column": "region", "op": "eq", "value": "AMER" } }
    ] });
    let out = run(
        json!({ "kind": "column", "x": { "column": "region", "bucket": null }, "measures": [{ "agg": "count" }] }),
        json!([]),
        ctx,
    )
    .unwrap();
    let filters: Vec<String> = out[0].1.filters.iter().map(|f| f.column.clone()).collect();
    // Same-table selection and the sheet-scoped slicer apply; the other table's table-scoped
    // selection and this visual's own selection don't.
    assert_eq!(filters, ["category", "store_id"]);
}

#[test]
fn drilling_into_a_year_then_a_quarter_shows_its_months() {
    let out = run(
        json!({ "kind": "column", "x": { "column": "order_date", "bucket": "year" }, "measures": [{ "agg": "count" }] }),
        json!(["2025-01-01", "2025-07-01"]),
        json!({}),
    )
    .unwrap();
    let q = serde_json::to_value(&out[0].1).unwrap();
    assert_eq!(
        q["dimensions"],
        json!([{ "column": "order_date", "bucket": "month" }])
    );
    assert_eq!(
        q["filters"],
        json!([
            { "column": "order_date", "op": "gte", "value": "2025-01-01" },
            { "column": "order_date", "op": "lt", "value": "2026-01-01" },
            { "column": "order_date", "op": "gte", "value": "2025-07-01" },
            { "column": "order_date", "op": "lt", "value": "2025-10-01" }
        ])
    );
}

#[test]
fn explicit_drill_paths_filter_by_equality() {
    let spec_json = json!({
        "kind": "bar",
        "x": { "column": "region", "bucket": null },
        "drill": [{ "column": "category", "bucket": null }, { "column": "store_id", "bucket": null }],
        "measures": [{ "column": "amount", "agg": "sum" }]
    });
    let out = run(spec_json.clone(), json!(["EMEA"]), json!({})).unwrap();
    let q = serde_json::to_value(&out[0].1).unwrap();
    assert_eq!(q["dimensions"][0]["column"], "category");
    assert_eq!(
        q["filters"],
        json!([{ "column": "region", "op": "eq", "value": "EMEA" }])
    );
    assert_eq!(
        run(spec_json, json!(["EMEA", "Outdoor", 7]), json!({})),
        Err(VisualError::DrillTooDeep)
    );
}

#[test]
fn kpi_compares_whole_months_with_the_previous_months() {
    let out = run(
        json!({
            "kind": "kpi",
            "measures": [{ "column": "amount", "agg": "sum" }],
            "options": { "compare": "order_date" }
        }),
        json!([]),
        json!({ "filters": [{ "column": "order_date", "op": "between", "value": ["2026-08-01", "2026-10-31"] }] }),
    )
    .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].0, QueryPart::Compare);
    let compare = serde_json::to_value(&out[1].1.filters).unwrap();
    assert_eq!(
        compare,
        json!([{ "column": "order_date", "op": "between", "value": ["2026-05-01", "2026-07-31"] }])
    );
}

#[test]
fn kpi_compares_day_ranges_by_length_and_skips_without_a_range() {
    let spec_json = json!({
        "kind": "kpi",
        "measures": [{ "agg": "count" }],
        "options": { "compare": "order_date" }
    });
    let out = run(
        spec_json.clone(),
        json!([]),
        json!({ "filters": [{ "column": "order_date", "op": "between", "value": ["2026-03-10", "2026-03-16"] }] }),
    )
    .unwrap();
    let compare = serde_json::to_value(&out[1].1.filters[0].value).unwrap();
    assert_eq!(compare, json!(["2026-03-03", "2026-03-09"]));
    assert_eq!(run(spec_json, json!([]), json!({})).unwrap().len(), 1);
}

#[test]
fn raw_tables_page_and_specs_round_trip() {
    let s = spec(json!({ "kind": "table", "columns": ["order_date", "amount"] }));
    let out = compile(
        Uuid::nil(),
        &s,
        &[],
        Some(500),
        &SheetContext::default(),
        &orders(),
    )
    .unwrap();
    assert_eq!(out[0].query.offset, Some(500));
    assert_eq!(out[0].query.limit, Some(500));
    // Optional fields are omitted when serialised, so stored specs stay small and stable.
    let stored = serde_json::to_value(&s).unwrap();
    assert!(stored.get("x").is_none() && stored.get("sort").is_none());
    assert_eq!(serde_json::from_value::<VisualSpec>(stored).unwrap(), s);
}

#[test]
fn missing_fields_are_reported_per_kind() {
    for (kind, what) in [
        ("line", "an x axis"),
        ("pie", "an x axis"),
        ("kpi", "a measure"),
        ("heatmap", "an x axis"),
        ("scatter", "an x axis"),
    ] {
        let err = run(json!({ "kind": kind }), json!([]), json!({})).unwrap_err();
        assert!(
            matches!(err, VisualError::Missing { what: w, .. } if w == what),
            "{kind}: {err}"
        );
    }
}

#[test]
fn kpi_compares_to_date_ranges_and_the_same_period_last_year() {
    let kpi = |to: &str| {
        json!({
            "kind": "kpi",
            "measures": [{ "column": "amount", "agg": "sum" }],
            "options": { "compare": "order_date", "compare_to": to }
        })
    };
    let compared = |to: &str, from: &str, until: &str| {
        let ctx = json!({ "filters": [{ "column": "order_date", "op": "between", "value": [from, until] }] });
        let out = run(kpi(to), json!([]), ctx).unwrap();
        serde_json::to_value(&out[1].1.filters[0].value).unwrap()
    };
    // Fiscal quarter to date: the same stretch of the quarter before.
    assert_eq!(
        compared("previous_period", "2026-08-01", "2026-10-15"),
        json!(["2026-05-01", "2026-07-15"])
    );
    // Month to date on the 31st: the previous month, clamped to its last day.
    assert_eq!(
        compared("previous_period", "2026-10-01", "2026-10-31"),
        json!(["2026-09-01", "2026-09-30"])
    );
    assert_eq!(
        compared("previous_year", "2026-02-01", "2026-10-15"),
        json!(["2025-02-01", "2025-10-15"])
    );
    assert_eq!(
        compared("previous_year", "2028-02-01", "2028-02-29"),
        json!(["2027-02-01", "2027-02-28"])
    );
}

#[test]
fn fiscal_periods_drill_into_months() {
    let spec_json = json!({
        "kind": "column",
        "x": { "column": "order_date", "bucket": "fiscal_year" },
        "measures": [{ "column": "amount", "agg": "sum" }]
    });
    // Fiscal year → fiscal quarter → month → day.
    let out = run(spec_json.clone(), json!(["2026-02-01"]), json!({})).unwrap();
    let q = &out[0].1;
    assert_eq!(
        serde_json::to_value(&q.dimensions[0]).unwrap(),
        json!({ "column": "order_date", "bucket": "fiscal_quarter" })
    );
    assert_eq!(
        serde_json::to_value(&q.filters).unwrap(),
        json!([
            { "column": "order_date", "op": "gte", "value": "2026-02-01" },
            { "column": "order_date", "op": "lt", "value": "2027-02-01" }
        ])
    );
    let out = run(spec_json, json!(["2026-02-01", "2026-08-01"]), json!({})).unwrap();
    assert_eq!(
        serde_json::to_value(&out[0].1.filters[2..]).unwrap(),
        json!([
            { "column": "order_date", "op": "gte", "value": "2026-08-01" },
            { "column": "order_date", "op": "lt", "value": "2026-11-01" }
        ])
    );
}
