/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Token bucket management

use crate::token_bucket::TokenBucket;
use aws_smithy_http::result::SdkError;
use futures_util::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// A service that wraps another service, adding the ability to set a quota for requests
/// handled by the inner service.
#[derive(Clone, Debug)]
pub struct QuotaService<S: 'static, Tb: TokenBucket> {
    inner: S,
    token_bucket: Tb,
}

impl<S, Tb: TokenBucket> QuotaService<S, Tb> {
    /// Create a new `QuotaService`
    pub fn new(inner: S, token_bucket: Tb) -> Self {
        Self {
            inner,
            token_bucket,
        }
    }
}

impl<S, Req, Tb, E> Service<Req> for QuotaService<S, Tb>
where
    S: Service<Req, Error = SdkError<E>>,
    S::Response: 'static,
    S::Future: 'static,
    E: 'static,
    Tb: TokenBucket,
{
    type Response = S::Response;
    type Error = SdkError<E>;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Check the inner service to see if it's ready yet. If no tokens are available, requests
        // should fail with an error instead of waiting for the next token.
        self.inner.poll_ready(cx).map_err(|_err| {
            SdkError::construction_failure(format!("inner service failed to become ready"))
        })
    }

    fn call(&mut self, mut req: Req) -> Self::Future {
        match self.token_bucket.try_acquire(None) {
            Ok(token) => {
                // req.properties_mut().insert(token);
                let fut = self.inner.call(req);

                Box::pin(fut)
            }
            Err(err) => {
                let fut = futures_util::future::err::<_, SdkError<E>>(
                    SdkError::construction_failure(err),
                );

                Box::pin(fut)
            }
        }
    }
}

/// A layer that wraps services in a quota service
#[non_exhaustive]
#[derive(Debug)]
pub struct QuotaLayer<Tbb> {
    token_bucket_builder: Tbb,
}

impl<Tb, TbBuilder> QuotaLayer<TbBuilder>
where
    Tb: TokenBucket,
    TbBuilder: Fn() -> Tb,
{
    /// Create a new `QuotaLayer`
    pub fn new(token_bucket_builder: TbBuilder) -> Self {
        QuotaLayer {
            token_bucket_builder,
        }
    }
}

impl<S, Tb, TbBuilder> Layer<S> for QuotaLayer<TbBuilder>
where
    S: 'static,
    Tb: TokenBucket,
    TbBuilder: Fn() -> Tb,
{
    type Service = QuotaService<S, Tb>;

    fn layer(&self, inner: S) -> Self::Service {
        QuotaService::new(inner, (self.token_bucket_builder)())
    }
}
//
// #[cfg(test)]
// mod tests {
//     use super::QuotaService;
//     use crate::token_bucket::standard;
//     use crate::token_bucket::TokenBucket;
//     use aws_smithy_http::body::SdkBody;
//     use aws_smithy_http::operation::Operation;
//     use aws_smithy_http::result::SdkError;
//     use aws_smithy_types::retry::ErrorKind;
//     use futures_util::future::TryFutureExt;
//     use http::{Request, Response, StatusCode};
//     use std::future::Future;
//     use std::marker::PhantomData;
//     use std::pin::Pin;
//     use std::task::{Context, Poll};
//     use std::time::Duration;
//     use tower::{Service, ServiceExt};
//
//     #[derive(Clone)]
//     struct TestService<H, R> {
//         handler: PhantomData<H>,
//         retry: PhantomData<R>,
//     }
//
//     impl<H, R> TestService<H, R> {
//         pub fn new() -> Self {
//             Self {
//                 handler: PhantomData::default(),
//                 retry: PhantomData::default(),
//             }
//         }
//     }
//
//     impl<H, R> Service<Operation<H, R>> for TestService<H, R> {
//         type Response = Response<&'static str>;
//         type Error = SdkError<()>;
//         type Future =
//             Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + Sync>>;
//
//         fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//             Poll::Ready(Ok(()))
//         }
//
//         fn call(&mut self, _req: Operation<H, R>) -> Self::Future {
//             let fut = async {
//                 Ok(Response::builder()
//                     .status(StatusCode::OK)
//                     .body("Hello, world!")
//                     .unwrap())
//             };
//
//             Box::pin(fut)
//         }
//     }
//
//     #[tokio::test]
//     async fn quota_service_has_ready_trait_method() {
//         let mut svc = QuotaService::new(
//             TestService::<(), ()>::new(),
//             standard::TokenBucket::builder().build(),
//         );
//
//         let _mut_ref = svc.ready().await.unwrap();
//     }
//
//     #[tokio::test]
//     async fn quota_service_is_send_sync() {
//         fn check_send_sync<T: Send + Sync>(t: T) -> T {
//             t
//         }
//
//         let svc = QuotaService::new(
//             TestService::<(), ()>::new(),
//             standard::TokenBucket::builder().build(),
//         );
//
//         let _mut_ref = check_send_sync(svc).ready().await.unwrap();
//     }
//
//     #[tokio::test]
//     async fn quota_layer_keeps_working_after_getting_emptied_and_then_refilled() {
//         let quota_state = standard::TokenBucket::builder()
//             .max_tokens(500)
//             .retryable_error_cost(5)
//             .timeout_error_cost(10)
//             .starting_tokens(10)
//             .build();
//         assert_eq!(quota_state.available(), 10);
//         // Remove the only token in the bucket, from the bucket
//         let the_only_token_in_the_bucket = quota_state
//             .try_acquire(Some(ErrorKind::TransientError))
//             .unwrap();
//         assert_eq!(quota_state.available(), 0);
//
//         let mut svc = QuotaService::new(TestService::new(), quota_state);
//
//         let req = Request::builder()
//             .body(SdkBody::empty())
//             .expect("failed to construct empty request");
//         let req = aws_smithy_http::operation::Request::new(req);
//         let op = Operation::new(req, ());
//
//         let op_clone = op.try_clone().unwrap();
//         let svc_clone = svc.clone();
//         let handle_a = tokio::task::spawn(async move {
//             let mut svc = svc_clone;
//             let _ = svc.ready().await;
//             svc.call(op_clone).await
//         });
//
//         // We need to make sure that the task has time to check readiness and find that the token
//         // bucket is empty.
//         tokio::time::sleep(Duration::from_secs(1)).await;
//
//         // Relinquish the semaphore token we held, enabling future requests to succeed.
//         drop(the_only_token_in_the_bucket);
//         let res_a = handle_a.await.expect("join handle is valid");
//         let res_b = svc.ready().and_then(|f| f.call(op)).await;
//
//         println!("{res_a:#?}, {res_b:#?}");
//     }
// }
