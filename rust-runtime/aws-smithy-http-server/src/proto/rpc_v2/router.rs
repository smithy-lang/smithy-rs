/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::body::empty;
use crate::body::BoxBody;
use crate::extension::RuntimeErrorExtension;
use crate::proto::rest::router::Error;
use crate::response::IntoResponse;
use crate::routing::Router;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::RpcV2;

pub use crate::proto::rest::router::*;

pub struct RpcV2Router;

// TODO(rpcv2): Implement (current body copied from the rest xml impl)
// and document.
/// A Smithy RPC V2 routing error.
impl IntoResponse<RpcV2> for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        match self {
            Error::NotFound => http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "application/xml")
                .extension(RuntimeErrorExtension::new(
                    UNKNOWN_OPERATION_EXCEPTION.to_string(),
                ))
                .body(empty())
                .expect("invalid HTTP response for REST XML routing error; please file a bug report under https://github.com/awslabs/smithy-rs/issues"),
            Error::MethodNotAllowed => method_disallowed(),
        }
    }
}

impl<B> Router<B> for RpcV2Router {
    type Service = ();

    type Error = ();

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
        todo!()
    }
}

impl FromIterator<(String, ())> for RpcV2Router {
    fn from_iter<T: IntoIterator<Item = (String, ())>>(iter: T) -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue, Method};

    use crate::proto::test_helpers::req;

    use super::{Router, RpcV2Router};

    #[tokio::test]
    async fn simple_routing() {
        let routes = vec!["Service.Operation"];
        let router: RpcV2Router = routes.clone().into_iter().map(|op| (op.to_owned(), ())).collect();

        let mut headers = HeaderMap::new();
        headers.insert("x-amz-target", HeaderValue::from_static("Service.Operation"));

        // Request should match
        let res = router.match_route(&req(&Method::GET, "/", Some(headers)));
        assert!(res.is_ok());
    }
}
