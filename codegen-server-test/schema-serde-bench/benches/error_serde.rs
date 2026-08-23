//! Criterion wall-time benches: legacy `IntoResponse<P>` vs schema
//! `ServerProtocol::serialize_error`, full response assembly (status + headers
//! + body) with the body drained so neither side can defer work.
//!
//! The legacy path consumes the operation-error enum, so its input is cloned
//! in the (unmeasured) batch setup; the schema path serializes from a shared
//! reference and needs no setup.

use aws_smithy_http_server::body::BoxBody;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use schema_serde_bench::drain;

fn bench_case<E: Clone>(
    c: &mut Criterion,
    name: &str,
    error: E,
    legacy: fn(E) -> http::Response<BoxBody>,
    schema: fn(&E) -> http::Response<BoxBody>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let mut group = c.benchmark_group(name);
    group.bench_function("legacy", |b| {
        b.to_async(&rt).iter_batched(
            || error.clone(),
            |e| async move { drain(legacy(e)).await },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("schema", |b| {
        b.to_async(&rt)
            .iter(|| async { drain(schema(&error)).await })
    });
    group.finish();
}

fn benches(c: &mut Criterion) {
    {
        use schema_serde_bench::validation_exception as case;
        bench_case(
            c,
            "restjson1_validation_exception",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::complex_error_header as case;
        bench_case(
            c,
            "restjson1_complex_error_header_split",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::awsjson11_invalid_greeting as case;
        bench_case(
            c,
            "awsjson11_invalid_greeting",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::rpcv2cbor_invalid_greeting as case;
        bench_case(
            c,
            "rpcv2cbor_invalid_greeting",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
}

criterion_group!(error_serde, benches);
criterion_main!(error_serde);
