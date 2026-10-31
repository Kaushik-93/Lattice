//! Every REST response builds a Server-Timing header, so it must stay far below a microsecond.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use lattice_telemetry::ServerTiming;

fn bench(c: &mut Criterion) {
    c.bench_function("server_timing/record3_and_render", |b| {
        b.iter(|| {
            let t = ServerTiming::new();
            t.record("auth", black_box(Duration::from_micros(420)));
            t.record("db", black_box(Duration::from_micros(1070)));
            t.record("db", black_box(Duration::from_micros(230)));
            black_box(t.header_value(Duration::from_micros(1810)))
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
