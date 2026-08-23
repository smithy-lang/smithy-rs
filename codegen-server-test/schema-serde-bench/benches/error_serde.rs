//! Criterion wall-time benches: legacy `IntoResponse<P>` (flag-OFF crate) vs
//! the schema-driven `IntoResponse<P>` (flag-ON `*-schema` crate, delegating to
//! `ServerProtocol::serialize_error`), full response assembly (enum dispatch +
//! status + headers + body) with the body drained so neither side can defer
//! work.
//!
//! Both paths consume their operation-error enum, so inputs are cloned in the
//! (unmeasured) batch setup on both sides — the comparison is symmetric.

use aws_smithy_http_server::body::BoxBody;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use schema_serde_bench::drain;

fn bench_case<L: Clone, S: Clone>(
    c: &mut Criterion,
    name: &str,
    legacy_error: L,
    schema_error: S,
    legacy: fn(L) -> http::Response<BoxBody>,
    schema: fn(S) -> http::Response<BoxBody>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let mut group = c.benchmark_group(name);
    group.bench_function("legacy", |b| {
        b.to_async(&rt).iter_batched(
            || legacy_error.clone(),
            |e| async move { drain(legacy(e)).await },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("schema", |b| {
        b.to_async(&rt).iter_batched(
            || schema_error.clone(),
            |e| async move { drain(schema(e)).await },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

fn benches(c: &mut Criterion) {
    {
        use schema_serde_bench::validation_exception as case;
        bench_case(
            c,
            "restjson1_validation_exception",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::complex_error_header as case;
        bench_case(
            c,
            "restjson1_complex_error_header_split",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::awsjson11_invalid_greeting as case;
        bench_case(
            c,
            "awsjson11_invalid_greeting",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::rpcv2cbor_invalid_greeting as case;
        bench_case(
            c,
            "rpcv2cbor_invalid_greeting",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
}

criterion_group!(error_serde, benches);
criterion_main!(error_serde);
