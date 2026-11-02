//! The semantic compiler on a model shaped like the demo: SQL snapshots for the shapes that
//! matter, and the errors the harness relies on to refuse bad definitions.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use chrono::NaiveDate;
use lattice_planner::{FilterOp, FilterValue, Scalar, SortDir, TimeBucket};
use lattice_semantic::ast::Path;
use lattice_semantic::{
    BoundTable, Column, Meta, Model, RawDefinition, SemDimension, SemFilter, SemanticQuery,
    Standing, check_metric, compile,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn table(n: u128, name: &str, cols: &[(&str, &str)]) -> BoundTable {
    BoundTable {
        table_id: Uuid::from_u128(n),
        reference: format!("\"project\".\"s\".\"{name}\""),
        columns: cols
            .iter()
            .map(|(c, t)| Column {
                name: (*c).into(),
                data_type: (*t).into(),
            })
            .collect(),
        data_version: 3,
    }
}

struct Note {
    id: u128,
    kind: &'static str,
    title: &'static str,
    def: Value,
    standing: Standing,
}

fn note(id: u128, kind: &'static str, title: &'static str, def: Value) -> Note {
    Note {
        id,
        kind,
        title,
        def,
        standing: Standing::Approved,
    }
}

fn demo(extra: Vec<Note>) -> Model {
    let mut notes = vec![
        note(1, "entity", "Order", Value::Null),
        note(2, "entity", "Customer", Value::Null),
        note(3, "entity", "Store", Value::Null),
        note(4, "entity", "Product", Value::Null),
        note(
            10,
            "relationship",
            "Order placed by Customer",
            json!({ "from": "order.customer_id", "to": "customer.customer_id" }),
        ),
        note(
            11,
            "relationship",
            "Order sold at Store",
            json!({ "from": "order.store_id", "to": "store.store_id" }),
        ),
        note(
            20,
            "rule",
            "Settled orders only",
            json!({ "condition": "order.status = 'settled'" }),
        ),
        note(
            21,
            "rule",
            "Exclude internal accounts",
            json!({ "condition": "customer.is_internal = FALSE", "always": true }),
        ),
        note(
            22,
            "rule",
            "Fiscal calendar",
            json!({ "fiscal_year_start_month": 2 }),
        ),
        note(
            30,
            "metric",
            "Net revenue",
            json!({ "formula": "SUM(order.amount) WHERE [[Settled orders only]]" }),
        ),
        note(
            31,
            "metric",
            "Average order value",
            json!({ "formula": "[[Net revenue]] / COUNT(order) WHERE [[Settled orders only]]" }),
        ),
        note(
            32,
            "metric",
            "Active customers",
            json!({ "formula": "COUNT DISTINCT(order.customer_id) WHERE [[Settled orders only]] AND order.order_date >= TODAY - 90 DAYS" }),
        ),
        note(
            33,
            "metric",
            "Customers",
            json!({ "formula": "COUNT(customer)" }),
        ),
    ];
    notes.extend(extra);
    let tables = [
        (
            1u128,
            table(
                100,
                "orders",
                &[
                    ("order_id", "BIGINT"),
                    ("order_date", "DATE"),
                    ("customer_id", "BIGINT"),
                    ("store_id", "INTEGER"),
                    ("status", "VARCHAR"),
                    ("amount", "DECIMAL(12,2)"),
                    ("region", "VARCHAR"),
                ],
            ),
        ),
        (
            2,
            table(
                101,
                "customers",
                &[
                    ("customer_id", "BIGINT"),
                    ("segment", "VARCHAR"),
                    ("is_internal", "BOOLEAN"),
                ],
            ),
        ),
        (
            3,
            table(
                102,
                "stores",
                &[
                    ("store_id", "INTEGER"),
                    ("region", "VARCHAR"),
                    ("city", "VARCHAR"),
                ],
            ),
        ),
    ];
    Model::build(
        notes.iter().map(|n| RawDefinition {
            meta: Meta {
                note_id: Uuid::from_u128(n.id),
                title: n.title.into(),
                version: 1,
                standing: n.standing,
            },
            kind: n.kind,
            definition: if n.def.is_null() { None } else { Some(&n.def) },
        }),
        |id| {
            tables
                .iter()
                .find(|(n, _)| Uuid::from_u128(*n) == id)
                .map(|(_, t)| t.clone())
        },
    )
}

fn query(metrics: &[u128], dims: Vec<SemDimension>) -> SemanticQuery {
    SemanticQuery {
        metrics: metrics.iter().map(|m| Uuid::from_u128(*m)).collect(),
        dimensions: dims,
        filters: vec![],
        context_filters: vec![],
        sort: vec![],
        limit: None,
        today: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        top: None,
    }
}

fn dim(entity: &str, column: &str, bucket: Option<TimeBucket>) -> SemDimension {
    SemDimension {
        path: Path {
            entity: entity.into(),
            column: column.into(),
        },
        bucket,
        bin: None,
    }
}

fn render(model: &Model, q: &SemanticQuery) -> String {
    let c = compile(model, q).expect("compiles");
    format!(
        "{}\n-- params: {:?}\n-- grain: {} · logic: {} · tables: {:?}",
        c.query.sql,
        c.query.params,
        c.grain,
        c.logic,
        c.data_versions
            .iter()
            .map(|(t, _)| t.as_u128())
            .collect::<Vec<_>>()
    )
}

#[test]
fn net_revenue_by_store_region_and_month() {
    let model = demo(vec![]);
    let q = query(
        &[30],
        vec![
            dim("store", "region", None),
            dim("order", "order_date", Some(TimeBucket::Month)),
        ],
    );
    // Joins customers too: "Exclude internal accounts" stands on every query reaching Customer.
    insta::assert_snapshot!(render(&model, &q));
}

#[test]
fn composite_metrics_and_relative_dates() {
    let model = demo(vec![]);
    insta::assert_snapshot!(render(
        &model,
        &query(&[31, 32], vec![dim("order", "region", None)])
    ));
}

#[test]
fn filters_apply_where_they_can() {
    let model = demo(vec![]);
    let mut q = query(&[30], vec![]);
    q.filters = vec![SemFilter {
        column: "region".into(),
        op: FilterOp::In,
        value: Some(FilterValue::Many(vec![Scalar::Text("EMEA".into())])),
    }];
    q.context_filters = vec![
        SemFilter {
            column: "customer.segment".into(),
            op: FilterOp::Eq,
            value: Some(FilterValue::One(Scalar::Text("business".into()))),
        },
        // Not reachable from orders: skipped, not an error.
        SemFilter {
            column: "product.category".into(),
            op: FilterOp::Eq,
            value: Some(FilterValue::One(Scalar::Text("Toys".into()))),
        },
        SemFilter {
            column: "order_date".into(),
            op: FilterOp::Between,
            value: Some(FilterValue::Many(vec![
                Scalar::Text("2026-08-01".into()),
                Scalar::Text("2026-10-31".into()),
            ])),
        },
    ];
    q.sort = vec![("net_revenue".into(), SortDir::Desc)];
    insta::assert_snapshot!(render(&model, &q));
}

#[test]
fn checks_report_what_is_wrong() {
    let bad = |formula: &str| {
        let model = demo(vec![note(
            90,
            "metric",
            "Bad",
            json!({ "formula": formula }),
        )]);
        check_metric(&model, Uuid::from_u128(90)).unwrap_err().0
    };
    assert!(
        bad("SUM(order.amount) + COUNT(customer)")
            .contains("mixes aggregates over order and customer")
    );
    assert!(bad("SUM(product.price)").contains("isn't connected to a table"));
    assert!(bad("SUM(order.amout)").contains("no column \"amout\""));
    assert!(bad("SUM(order.status)").contains("needs a number"));
    assert!(bad("SUM(order.amount) WHERE order.status = 3").contains("different types"));
    assert!(bad("SUM(shipment.cost)").contains("no entity called \"shipment\""));
    assert!(bad("[[Bad]] + 1").contains("refers to itself"));
    assert!(bad("[[Nope]]").contains("no metric called \"Nope\""));
    assert!(bad("SUM(order.amount) WHERE [[Nope]]").contains("no rule called"));
    assert!(bad("SUM(order.amount").contains("expected"));

    let model = demo(vec![]);
    let ok = check_metric(&model, Uuid::from_u128(31)).unwrap();
    assert_eq!(ok.grain, "order");
    assert_eq!(
        ok.uses.len(),
        3,
        "AOV uses itself, Net revenue and the settled rule"
    );

    // A customer-grain query can't reach stores: relationships only go many-to-one.
    let err = compile(&model, &query(&[33], vec![dim("store", "region", None)])).unwrap_err();
    assert!(err.0.contains("can't relate customer to store"), "{err}");
    // Metrics over different grains don't share a query.
    let err = compile(&model, &query(&[30, 33], vec![])).unwrap_err();
    assert!(err.0.contains("mixes aggregates"), "{err}");
}

#[test]
fn unapproved_definitions_are_reported_for_badges() {
    let mut draft = note(40, "metric", "Refund rate", json!({ "formula": "CAST_ME" }));
    draft.def =
        json!({ "formula": "(COUNT(order) WHERE order.status = 'refunded') / COUNT(order)" });
    draft.standing = Standing::Unapproved;
    let model = demo(vec![draft]);
    let c = compile(&model, &query(&[40], vec![])).unwrap();
    assert_eq!(c.unapproved, vec![Uuid::from_u128(40)]);
    assert!(c.query.sql.contains("NULLIF"));
    // Standing rules only apply once approved.
    let mut model = demo(vec![]);
    model
        .rules
        .iter_mut()
        .for_each(|r| r.meta.standing = Standing::Unapproved);
    let c = compile(&model, &query(&[33], vec![])).unwrap();
    assert!(!c.query.sql.contains("is_internal"));
    assert_eq!(demo(vec![]).fiscal_year_start_month, 2);
}
