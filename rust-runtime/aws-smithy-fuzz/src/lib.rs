/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
#![cfg(not(windows))]
use libloading::{os, Library, Symbol};
use std::error::Error;
use tokio::sync::mpsc::Sender;

use futures::task::noop_waker;
use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tower::ServiceExt;
mod types;
pub use lazy_static;
pub use types::{Body, FuzzResult, HttpRequest, HttpResponse};

#[macro_export]
/// Defines an extern `process_request` method that can be invoked as a shared library
macro_rules! fuzz_harness {
    ($test_function: expr) => {
        $crate::lazy_static::lazy_static! {
            static ref TARGET: std::sync::Mutex<$crate::LocalFuzzTarget> = $crate::make_target($test_function);
        }

        #[no_mangle]
        pub extern "C" fn process_request(input: *const u8, len: usize) -> $crate::ByteBuffer {
            let slice = unsafe { std::slice::from_raw_parts(input, len) };
            let request = $crate::HttpRequest::from_bytes(slice);
            let response = TARGET.lock().unwrap().invoke(
                request
                    .into_http_request_04x()
                    .expect("input was not a valid HTTP request. Bug in driver."),
            );
            $crate::ByteBuffer::from_vec(response.into_bytes())
        }
    };
}

pub use ::ffi_support::ByteBuffer;
use bytes::Buf;
use tokio::runtime::{Builder, Handle};
use tower::util::BoxService;

#[derive(Clone)]
pub struct FuzzTarget(os::unix::Symbol<unsafe extern "C" fn(*const u8, usize) -> ByteBuffer>);
impl FuzzTarget {
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        eprintln!("loading library from {}", path.display());
        let library = unsafe { Library::new(path).expect("could not load library") };
        let func: Symbol<unsafe extern "C" fn(*const u8, usize) -> ByteBuffer> =
            unsafe { library.get(b"process_request").unwrap() };
        // ensure we never unload the library
        let func = unsafe { func.into_raw() };
        std::mem::forget(library);

        Self(func)
    }

    pub fn invoke_bytes(&self, input: &[u8]) -> FuzzResult {
        let buffer = unsafe { (self.0)(input.as_ptr(), input.len()) };
        let data = buffer.destroy_into_vec();
        FuzzResult::from_bytes(&data)
    }

    pub fn invoke(&self, request: &HttpRequest) -> FuzzResult {
        let input = request.as_bytes();
        self.invoke_bytes(&input)
    }
}

pub struct LocalFuzzTarget {
    service: BoxService<http::Request<Body>, http::Response<Vec<u8>>, Box<dyn Error + Send + Sync>>,
    rx: tokio::sync::mpsc::Receiver<String>,
}

impl LocalFuzzTarget {
    pub fn invoke(&mut self, request: http::Request<Body>) -> FuzzResult {
        assert_ready(async move {
            let result = ServiceExt::oneshot(&mut self.service, request)
                .await
                .unwrap();
            let debug = self.rx.try_recv().ok();
            if result.status().is_success() && debug.is_none() {
                panic!("success but no debug data received");
            }
            let (parts, body) = result.into_parts();
            FuzzResult {
                input: debug,
                response: HttpResponse {
                    status: parts.status.as_u16(),
                    headers: parts
                        .headers
                        .iter()
                        .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string()))
                        .collect(),
                    body,
                },
            }
        })
    }
}

/// Create a target from a tower service.
///
/// A `Sender<String>` is passed in. The service should send a deterministic string
/// based on the parsed input.
pub fn make_target<
    D: Send + Buf,
    B: http_body::Body<Data = D> + Send + 'static,
    F: Send + 'static,
    E: Into<Box<dyn Error + Send + Sync>>,
    T: tower::Service<http::Request<Body>, Response = http::Response<B>, Future = F, Error = E>
        + Send
        + 'static,
>(
    service: impl Fn(Sender<String>) -> T,
) -> Mutex<LocalFuzzTarget> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let service =
        service(tx)
            .map_err(|e| e.into())
            .and_then(|resp: http::Response<B>| async move {
                let (parts, body) = resp.into_parts();
                let body = body.collect().await.ok().unwrap().to_bytes().to_vec();
                Ok::<_, Box<dyn Error + Send + Sync>>(http::Response::from_parts(parts, body))
            });
    let service = BoxService::new(service);
    Mutex::new(LocalFuzzTarget { service, rx })
}

#[allow(unused)]
fn assert_ready_tokio<F: Future>(future: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) => handle.block_on(future),
        Err(_) => {
            let handle = Builder::new_multi_thread().build().unwrap();
            handle.block_on(future)
        }
    }
}

/// Polls a future and panics if it isn't already ready.
fn assert_ready<F: Future>(mut future: F) -> F::Output {
    // Create a waker that does nothing.
    let waker = noop_waker();
    // Create a context from the waker.
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => {
            panic!("poll pending...")
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{make_target, Body};
    use http::Request;
    use std::error::Error;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc::Sender;

    fuzz_harness!(|_tx| { TestService });

    fn make_service(_sender: Sender<String>) -> TestService {
        TestService
    }

    #[test]
    fn test() {
        make_target(make_service);
    }

    struct TestService;

    impl tower::Service<http::Request<Body>> for TestService {
        type Response = http::Response<Body>;
        type Error = Box<dyn Error + Send + Sync>;
        type Future =
            Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + Sync>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            todo!()
        }

        fn call(&mut self, _req: Request<Body>) -> Self::Future {
            todo!()
        }
    }
}
