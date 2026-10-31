//! Session-token work happens on every login and every authenticated request; Argon2
//! verification bounds login latency (and is intentionally slow).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use lattice_security::password::{hash_password, verify_password};
use lattice_security::token::{SessionToken, hash_token};

fn bench(c: &mut Criterion) {
    c.bench_function("token/generate", |b| b.iter(SessionToken::generate));

    let token = SessionToken::generate();
    c.bench_function("token/hash_for_lookup", |b| {
        b.iter(|| hash_token(black_box(token.as_str())))
    });

    #[allow(clippy::expect_used)]
    let phc = hash_password("benchmark-password").expect("hashing works");
    let mut group = c.benchmark_group("password");
    group.sample_size(20);
    group.bench_function("argon2id_verify", |b| {
        b.iter(|| verify_password(black_box("benchmark-password"), &phc))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
