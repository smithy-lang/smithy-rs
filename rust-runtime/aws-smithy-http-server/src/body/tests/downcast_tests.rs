/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::super::{boxed, try_downcast, Body, BodyInner, BoxBody};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// Count only allocations made on the measuring thread during the synchronous boxing call.
thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(current) = count.get() {
            count.set(Some(current + 1));
        }
    });
}

// SAFETY: Each allocation operation delegates unchanged to System, including its layout
// and pointer arguments. The thread-local counter does not allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocation_count<T>(f: impl FnOnce() -> T) -> (T, usize) {
    struct ResetCounter;
    impl Drop for ResetCounter {
        fn drop(&mut self) {
            ALLOCATIONS.with(|count| count.set(None));
        }
    }

    ALLOCATIONS.with(|count| {
        assert!(count.get().is_none(), "allocation measurement must not be nested");
        count.set(Some(0));
    });
    let reset = ResetCounter;
    let result = f();
    let count = ALLOCATIONS.with(|count| count.get().unwrap());
    drop(reset);
    (result, count)
}

#[tokio::test]
async fn boxing_an_unboxed_body_allocates_and_preserves_its_contents() {
    let bytes = Bytes::from_static(b"unboxed payload");
    let original = Full::new(bytes.clone());
    let (body, allocations): (BoxBody, _) = allocation_count(|| boxed(original));
    assert!(allocations > 0, "an unboxed body needs an allocation");
    assert_eq!(body.collect().await.unwrap().to_bytes(), bytes);
}

#[tokio::test]
async fn boxing_an_existing_box_body_does_not_allocate() {
    let bytes = Bytes::from_static(b"already boxed payload");
    let original: BoxBody = boxed(Full::new(bytes.clone()));
    let (body, allocations): (BoxBody, _) = allocation_count(|| boxed(original));
    assert_eq!(allocations, 0, "an existing BoxBody must not be boxed again");
    assert_eq!(body.collect().await.unwrap().to_bytes(), bytes);
}

#[tokio::test]
async fn wrapping_a_request_body_preserves_its_buffered_state_and_storage() {
    let bytes = Bytes::from_static(b"payload");
    let mut trailers = http::HeaderMap::new();
    trailers.insert("checksum", http::HeaderValue::from_static("abc"));
    let original = Body::buffered(bytes.clone(), Some(trailers.clone()));
    let mut body = Body::new(original);
    assert!(matches!(body.0, BodyInner::Buffered { tail: None, .. }));
    assert_eq!(body.buffered_content().unwrap().as_ptr(), bytes.as_ptr());
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), bytes);
    assert_eq!(body.frame().await.unwrap().unwrap().into_trailers().unwrap(), trailers);
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn wrapping_a_partially_buffered_request_body_preserves_its_tail() {
    let original = Body::prefixed(
        Bytes::from_static(b"prefix"),
        None,
        Body::from_bytes(Bytes::from_static(b"tail")),
    );
    let body = Body::new(original);
    assert!(matches!(body.0, BodyInner::Buffered { tail: Some(_), .. }));
    assert!(body.buffered_content().is_none());
    assert_eq!(body.collect().await.unwrap().to_bytes(), "prefixtail");
}

#[tokio::test]
async fn wrapping_the_hyper_transport_body_keeps_it_unerased() {
    use std::convert::Infallible;
    use tokio::io::AsyncWriteExt;

    let (transport, mut client) = tokio::io::duplex(1024);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let service = hyper::service::service_fn(move |request: http::Request<hyper::body::Incoming>| {
        sender
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(request.into_body())
            .unwrap();
        async { Ok::<_, Infallible>(http::Response::new(Full::new(Bytes::new()))) }
    });
    let task = tokio::spawn(async move {
        hyper::server::conn::http1::Builder::new()
            .serve_connection(hyper_util::rt::TokioIo::new(transport), service)
            .await
    });
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\ntransport")
        .await
        .unwrap();
    let incoming = receiver.await.unwrap();
    let original = Body::new(incoming);
    assert!(matches!(original.0, BodyInner::Hyper(_)));
    let body = Body::new(original);
    assert!(matches!(body.0, BodyInner::Hyper(_)));
    assert_eq!(body.collect().await.unwrap().to_bytes(), "transport");
    task.abort();
}

#[tokio::test]
async fn wrapping_another_body_type_uses_the_boxed_fallback() {
    let bytes = Bytes::from_static(b"other source");
    let body = Body::new(Full::new(bytes.clone()));
    assert!(matches!(body.0, BodyInner::Boxed(_)));
    assert_eq!(body.collect().await.unwrap().to_bytes(), bytes);
}

#[test]
fn successful_and_failed_downcasts_preserve_ownership() {
    #[derive(Debug)]
    struct OwnedValue {
        value: String,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for OwnedValue {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    let drops = Arc::new(AtomicUsize::new(0));
    let original = OwnedValue {
        value: "owned value".into(),
        drops: drops.clone(),
    };
    let returned = try_downcast::<String, _>(original).unwrap_err();
    assert_eq!(returned.value, "owned value");
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let returned = try_downcast::<OwnedValue, _>(returned).unwrap();
    assert_eq!(returned.value, "owned value");
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(returned);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
