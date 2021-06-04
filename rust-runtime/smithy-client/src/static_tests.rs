//! This module provides types useful for static tests.
#![allow(missing_docs, missing_debug_implementations)]

use crate::*;

#[derive(Debug)]
#[non_exhaustive]
pub struct TestOperationError;
impl std::fmt::Display for TestOperationError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unreachable!("only used for static tests")
    }
}
impl Error for TestOperationError {}
impl ProvideErrorKind for TestOperationError {
    fn retryable_error_kind(&self) -> Option<smithy_types::retry::ErrorKind> {
        unreachable!("only used for static tests")
    }

    fn code(&self) -> Option<&str> {
        unreachable!("only used for static tests")
    }
}
#[derive(Clone)]
#[non_exhaustive]
pub struct TestOperation;
impl ParseHttpResponse<SdkBody> for TestOperation {
    type Output = Result<(), TestOperationError>;

    fn parse_unloaded(&self, _: &mut http::Response<SdkBody>) -> Option<Self::Output> {
        unreachable!("only used for static tests")
    }

    fn parse_loaded(&self, _response: &http::Response<bytes::Bytes>) -> Self::Output {
        unreachable!("only used for static tests")
    }
}
pub type ValidTestOperation = Operation<TestOperation, ()>;

// Statically check that a standard retry can actually be used to build a Client.
#[allow(dead_code)]
#[cfg(test)]
fn sanity_retry() {
    Builder::new()
        .middleware(tower::layer::util::Identity::new())
        .map_connector(|_| async { unreachable!() })
        .build()
        .check();
}

// Statically check that a hyper client can actually be used to build a Client.
#[allow(dead_code)]
#[cfg(all(test, feature = "hyper"))]
fn sanity_hyper<C>(hc: hyper::Client<C, SdkBody>)
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    Builder::new()
        .middleware(tower::layer::util::Identity::new())
        .hyper(hc)
        .build()
        .check();
}

// Statically check that a type-erased middleware client is actually a valid Client.
#[allow(dead_code)]
fn sanity_erase_middleware() {
    Builder::new()
        .middleware(tower::layer::util::Identity::new())
        .map_connector(|_| async { unreachable!() })
        .build()
        .erase_middleware()
        .check();
}

// Statically check that a type-erased connector client is actually a valid Client.
#[allow(dead_code)]
fn sanity_erase_connector() {
    Builder::new()
        .middleware(tower::layer::util::Identity::new())
        .map_connector(|_| async { unreachable!() })
        .build()
        .erase_connector()
        .check();
}
