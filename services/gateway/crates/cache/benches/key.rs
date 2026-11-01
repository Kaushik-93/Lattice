//! Every query hashes its cache key before anything else; it must stay negligible.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use lattice_cache::CacheKey;
use uuid::Uuid;

fn bench(c: &mut Criterion) {
    let sql = "SELECT make_date(\"k0_y\", \"k0_m\", 1) AS \"order_date_month\", \"m0\" AS \"sum_amount\" FROM (SELECT year(\"order_date\") AS \"k0_y\", month(\"order_date\") AS \"k0_m\", sum(\"amount\") AS \"m0\" FROM \"project\".\"s_demo\".\"orders\" WHERE \"region\" IN (?, ?) GROUP BY ALL) ORDER BY \"order_date_month\" ASC NULLS LAST LIMIT 10001";
    c.bench_function("cache/key_render", |b| {
        b.iter(|| {
            CacheKey {
                project_id: Uuid::nil(),
                sql: black_box(sql),
                params: black_box("[Text(\"EMEA\"), Text(\"APAC\")]"),
                data_version: 3,
                logic_version: 0,
                rls_context: "",
            }
            .render()
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
