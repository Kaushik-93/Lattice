//! Compilation runs on every query; it must stay in the low microseconds.

#![allow(clippy::expect_used)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use lattice_planner::{ColumnInfo, TableQuery, TableSchema, compile};

fn bench(c: &mut Criterion) {
    let table = TableSchema {
        reference: "\"project\".\"s_demo\".\"orders\"".into(),
        columns: [
            ("order_date", "DATE"),
            ("region", "VARCHAR"),
            ("category", "VARCHAR"),
            ("amount", "DECIMAL(12,2)"),
        ]
        .iter()
        .map(|(n, t)| ColumnInfo {
            name: (*n).into(),
            data_type: (*t).into(),
        })
        .collect(),
    };
    let query: TableQuery = serde_json::from_str(
        r#"{"table_id":"01930000-0000-7000-8000-000000000000",
            "dimensions":[{"column":"order_date","bucket":"month"},{"column":"region"}],
            "measures":[{"agg":"count"},{"column":"amount","agg":"sum"}],
            "filters":[{"column":"category","op":"in","value":["Outdoor","Apparel"]}],
            "sort":[{"by":"sum_amount","dir":"desc"}],"limit":100}"#,
    )
    .expect("query");
    c.bench_function("planner/compile_month_bucket_query", |b| {
        b.iter(|| compile(black_box(&query), black_box(&table)).expect("compiles"))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
