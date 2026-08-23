//! Heap-allocation comparison via dhat (valgrind-free, works on Windows):
//! for each golden case, runs N full response assemblies (serialize + drain)
//! on the legacy and schema paths and prints the per-iteration allocation
//! block/byte deltas from `dhat::HeapStats`.
//!
//! `cargo run --release --bin dhat_alloc`
//!
//! Inputs for the legacy path (which consumes its enum) are pre-cloned before
//! the measured region so input construction never lands in the deltas; dhat's
//! totals are cumulative, so drops inside the region do not subtract.

use aws_smithy_http_server::body::BoxBody;
use schema_serde_bench::drain;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const ITERS: usize = 1000;

fn measure<E: Clone>(
    rt: &tokio::runtime::Runtime,
    name: &str,
    error: E,
    legacy: fn(E) -> http::Response<BoxBody>,
    schema: fn(&E) -> http::Response<BoxBody>,
) {
    // Pre-clone legacy inputs outside the measured region.
    let inputs: Vec<E> = (0..ITERS).map(|_| error.clone()).collect();

    let before = dhat::HeapStats::get();
    rt.block_on(async {
        for e in inputs {
            std::hint::black_box(drain(legacy(e)).await);
        }
    });
    let after_legacy = dhat::HeapStats::get();

    rt.block_on(async {
        for _ in 0..ITERS {
            std::hint::black_box(drain(schema(&error)).await);
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
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::complex_error_header as case;
        measure(
            &rt,
            "restjson1_complex_error_header_split",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::awsjson11_invalid_greeting as case;
        measure(
            &rt,
            "awsjson11_invalid_greeting",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
    {
        use schema_serde_bench::rpcv2cbor_invalid_greeting as case;
        measure(
            &rt,
            "rpcv2cbor_invalid_greeting",
            case::error(),
            case::legacy,
            case::schema,
        );
    }
}
