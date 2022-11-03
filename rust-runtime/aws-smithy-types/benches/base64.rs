use criterion::{criterion_group, criterion_main, Criterion};

fn base64_simd(c: &mut Criterion) {
    c.bench_function("Our base64 encode", |b| {
        b.iter(|| {
            let _encoded = aws_smithy_types::base64::encode(b"something");
        })
    });
}

fn base64(c: &mut Criterion) {
    c.bench_function("Our base64 encode", |b| {
        b.iter(|| {
            let _encoded = aws_smithy_types::base64::encode(b"something");
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = base64, base64_simd
}

criterion_main!(benches);
