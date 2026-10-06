use bytes::Bytes;
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use std::{convert::Infallible, hint::black_box, time::{Duration, Instant}};
use tower::{Service, ServiceExt};

#[cfg(feature = "allocations")]
mod allocations {
    use std::{alloc::{GlobalAlloc, Layout, System}, sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed}};
    pub struct Counting;
    pub static ENABLED: AtomicBool = AtomicBool::new(false);
    pub static CALLS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub static LIVE: AtomicI64 = AtomicI64::new(0);
    pub static PEAK: AtomicI64 = AtomicI64::new(0);
    fn allocated(size: usize) {
        CALLS.fetch_add(1, Relaxed);
        BYTES.fetch_add(size as u64, Relaxed);
        let live = LIVE.fetch_add(size as i64, Relaxed) + size as i64;
        PEAK.fetch_max(live, Relaxed);
    }
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() && ENABLED.load(Relaxed) { allocated(layout.size()); }
            ptr
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() && ENABLED.load(Relaxed) { allocated(layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if ENABLED.load(Relaxed) { LIVE.fetch_sub(layout.size() as i64, Relaxed); }
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
            let result = unsafe { System.realloc(ptr, old, size) };
            if !result.is_null() && ENABLED.load(Relaxed) {
                LIVE.fetch_sub(old.size() as i64, Relaxed);
                allocated(size);
            }
            result
        }
    }
}
#[cfg(feature = "allocations")]
#[global_allocator]
static ALLOCATOR: allocations::Counting = allocations::Counting;

fn process_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) }, 0);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn payload(protocol: &str, value: &str) -> Bytes {
    match protocol {
        "rest_xml" => format!("<EchoInput><value>{value}</value></EchoInput>").into(),
        "rpc_v2_cbor" => {
            let mut bytes = Vec::new();
            ciborium::into_writer(&std::collections::BTreeMap::from([("value", value)]), &mut bytes).unwrap();
            bytes.into()
        }
        _ => serde_json::to_vec(&serde_json::json!({"value": value})).unwrap().into(),
    }
}
fn request(protocol: &str, payload: &Bytes) -> Request<Full<Bytes>> {
    let (path, content_type) = match protocol {
        "rest_json" => ("/echo", "application/json"),
        "rest_xml" => ("/echo", "application/xml"),
        "aws_json_10" => ("/", "application/x-amz-json-1.0"),
        "aws_json_11" => ("/", "application/x-amz-json-1.1"),
        "rpc_v2_cbor" => ("/service/BenchmarkService/operation/Echo", "application/cbor"),
        _ => panic!("unknown protocol"),
    };
    let mut builder = Request::builder().method("POST").uri(path).header("content-type", content_type);
    if protocol.starts_with("aws_json") { builder = builder.header("x-amz-target", "BenchmarkService.Echo"); }
    if protocol == "rpc_v2_cbor" { builder = builder.header("smithy-protocol", "rpc-v2-cbor"); }
    builder.body(Full::new(payload.clone())).unwrap()
}
async fn batch<S, B>(app: &S, protocol: &str, payload: &Bytes, iterations: usize)
where S: Service<Request<Full<Bytes>>, Response=Response<B>, Error=Infallible> + Clone,
      B: http_body::Body<Data=Bytes>, B::Error: std::fmt::Debug {
    for _ in 0..iterations {
        // Matches the adapter's clone + oneshot contract. No socket, HTTP parser, or TLS costs.
        let response = app.clone().oneshot(request(protocol, payload)).await.unwrap();
        black_box(response.into_body().collect().await.unwrap().to_bytes());
    }
}
fn run<S, B>(app: S, protocol: &str, size: usize)
where S: Service<Request<Full<Bytes>>, Response=Response<B>, Error=Infallible> + Clone,
      B: http_body::Body<Data=Bytes>, B::Error: std::fmt::Debug {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let value = "x".repeat(size);
    let payload = payload(protocol, &value);
    runtime.block_on(async {
        let response = app.clone().oneshot(request(protocol, &payload)).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, 200, "{protocol}: {}", String::from_utf8_lossy(&bytes));
        match protocol {
            "rest_xml" => assert!(String::from_utf8_lossy(&bytes).contains(&format!("<value>{value}</value>"))),
            "rpc_v2_cbor" => {
                let decoded: std::collections::BTreeMap<String, String> = ciborium::from_reader(bytes.as_ref()).unwrap();
                assert_eq!(decoded["value"], value);
            }
            _ => assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["value"], value),
        }
    });
    let warmup = Instant::now();
    while warmup.elapsed() < Duration::from_millis(300) {
        runtime.block_on(batch(&app, protocol, &payload, 100));
    }
    #[cfg(feature = "allocations")]
    {
        use std::sync::atomic::Ordering::Relaxed;
        runtime.block_on(async {
            allocations::ENABLED.store(true, Relaxed);
            batch(&app, protocol, &payload, 1000).await;
            allocations::ENABLED.store(false, Relaxed);
        });
        println!("{}", serde_json::json!({"protocol": protocol, "value_bytes": size,
            "allocations_per_request": allocations::CALLS.load(Relaxed) as f64 / 1000.0,
            "allocated_bytes_per_request": allocations::BYTES.load(Relaxed) as f64 / 1000.0,
            "peak_extra_live_heap_bytes": allocations::PEAK.load(Relaxed),
            "net_live_heap_bytes": allocations::LIVE.load(Relaxed)}));
    }
    #[cfg(not(feature = "allocations"))]
    {
        let start = Instant::now();
        runtime.block_on(batch(&app, protocol, &payload, 1000));
        let n = (20_000_000.0 / (start.elapsed().as_nanos() as f64 / 1000.0)).clamp(100.0, 100_000.0) as usize;
        let mut cpu = Vec::new();
        let mut wall = Vec::new();
        for _ in 0..40 {
            let start_cpu = process_ns();
            let start = Instant::now();
            runtime.block_on(batch(&app, protocol, &payload, n));
            wall.push(start.elapsed().as_nanos() as f64 / n as f64);
            cpu.push((process_ns() - start_cpu) as f64 / n as f64);
        }
        println!("{}", serde_json::json!({"protocol": protocol, "value_bytes": size,
            "iterations_per_sample": n, "cpu_ns_per_request": cpu, "wall_ns_per_request": wall}));
    }
}

// SERVER_MAIN
