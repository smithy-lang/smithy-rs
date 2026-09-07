use criterion::{criterion_group, criterion_main, Criterion};

fn bench_get_server_statistics(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
    let mut group = c.benchmark_group("get_server_statistics");

    for case in pokemon_service_benchmarks::CASES {
        let runner = case.runner();
        group.bench_function(runner.name, |b| b.to_async(&runtime).iter(|| runner.run()));
    }

    group.finish();
}

criterion_group!(benches, bench_get_server_statistics);
criterion_main!(benches);
