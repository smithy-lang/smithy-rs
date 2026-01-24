/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Outer middleware (Position A) - wraps the entire service.
//! This middleware sees ALL requests, even those that fail routing.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tower::{Layer, Service};

/// A Layer that wraps services with [`OuterMiddlewareService`].
#[derive(Clone, Debug)]
pub struct OuterMiddlewareLayer;

impl OuterMiddlewareLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for OuterMiddlewareLayer {
    type Service = OuterMiddlewareService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OuterMiddlewareService { inner }
    }
}

/// A service that wraps the entire application (Position A).
/// This is the outermost middleware - ALL requests pass through here.
#[derive(Clone, Debug)]
pub struct OuterMiddlewareService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for OuterMiddlewareService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        println!("\n[TRACE A1] ========== OUTER MIDDLEWARE (Position A) ==========");
        println!("[TRACE A1] File: pokemon-service/src/outer_middleware.rs");
        println!("[TRACE A1] Type: OuterMiddlewareService<S>");
        println!("[TRACE A1] This is the OUTERMOST layer - ALL requests pass through");
        println!("[TRACE A1] Method: {} | URI: {}", req.method(), req.uri());
        println!("[TRACE A1] ===========================================================\n");

        // Clone the service to avoid readiness issues
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            let result = inner.call(req).await;

            match &result {
                Ok(response) => {
                    println!("\n[TRACE A2] ========== OUTER MIDDLEWARE RESPONSE ==========");
                    println!("[TRACE A2] File: pokemon-service/src/outer_middleware.rs");
                    println!("[TRACE A2] Response Status: {}", response.status());
                    println!("[TRACE A2] Request completed successfully through outer middleware");
                    println!("[TRACE A2] ======================================================\n");
                }
                Err(_) => {
                    println!("\n[TRACE A2] OUTER MIDDLEWARE: Request resulted in error");
                }
            }

            result
        })
    }
}
