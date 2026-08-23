//! Heap-allocation comparison via dhat (valgrind-free, works on Windows):
//! for each golden case, runs N full response assemblies (serialize + drain)
//! on the legacy (flag-OFF crate) and schema (flag-ON `*-schema` crate) paths
//! and prints the per-iteration allocation block/byte deltas from
//! `dhat::HeapStats`.
//!
//! `cargo run --release --bin dhat_alloc`
//!
//! Both paths consume their operation-error enum; inputs are pre-cloned before
//! the measured region on both sides so input construction never lands in the
//! deltas; dhat's totals are cumulative, so drops inside the region do not
//! subtract.

use aws_smithy_http_server::body::BoxBody;
use schema_serde_bench::drain;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const ITERS: usize = 1000;

fn measure<L: Clone, S: Clone>(
    rt: &tokio::runtime::Runtime,
    name: &str,
    legacy_error: L,
    schema_error: S,
    legacy: fn(L) -> http::Response<BoxBody>,
    schema: fn(S) -> http::Response<BoxBody>,
) {
    // Pre-clone all inputs outside the measured region.
    let legacy_inputs: Vec<L> = (0..ITERS).map(|_| legacy_error.clone()).collect();
    let schema_inputs: Vec<S> = (0..ITERS).map(|_| schema_error.clone()).collect();

    let before = dhat::HeapStats::get();
    rt.block_on(async {
        for e in legacy_inputs {
            std::hint::black_box(drain(legacy(e)).await);
        }
    });
    let after_legacy = dhat::HeapStats::get();

    rt.block_on(async {
        for e in schema_inputs {
            std::hint::black_box(drain(schema(e)).await);
        }
    });
    let after_schema = dhat::HeapStats::get();

    let legacy_blocks = (after_legacy.total_blocks - before.total_blocks) as f64 / ITERS as f64;
    let legacy_bytes = (after_legacy.total_bytes - before.total_bytes) as f64 / ITERS as f64;
    let schema_blocks =
        (after_schema.total_blocks - after_legacy.total_blocks) as f64 / ITERS as f64;
    let schema_bytes = (after_schema.total_bytes - after_legacy.total_bytes) as f64 / ITERS as f64;

    println!("{name}");
    println!("  legacy: {legacy_blocks:8.1} allocs/iter  {legacy_bytes:10.1} bytes/iter");
    println!("  schema: {schema_blocks:8.1} allocs/iter  {schema_bytes:10.1} bytes/iter");
}

fn main() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    {
        use schema_serde_bench::validation_exception as case;
        measure(
            &rt,
            "restjson1_validation_exception",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::complex_error_header as case;
        measure(
            &rt,
            "restjson1_complex_error_header_split",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::awsjson11_invalid_greeting as case;
        measure(
            &rt,
            "awsjson11_invalid_greeting",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::rpcv2cbor_invalid_greeting as case;
        measure(
            &rt,
            "rpcv2cbor_invalid_greeting",
            case::legacy_error(),
            case::schema_error(),
            case::legacy,
            case::schema,
        );
    }
}
