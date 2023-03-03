/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use once_cell::sync::Lazy;
use regex::Regex;

use crate::body::empty;
use crate::body::BoxBody;
use crate::extension::RuntimeErrorExtension;
use crate::proto::aws_json_11::router::ROUTE_CUTOFF;
use crate::proto::rest::router::Error;
use crate::response::IntoResponse;
use crate::routing::tiny_map::TinyMap;
use crate::routing::Router;
use crate::routing::{method_disallowed, UNKNOWN_OPERATION_EXCEPTION};

use super::RpcV2;

pub use crate::proto::rest::router::*;

pub struct RpcV2Router<S> {
    routes: TinyMap<&'static str, S, ROUTE_CUTOFF>,
}

impl<S> RpcV2Router<S> {
    fn path_regex() -> &'static Regex {
        // TODO(rpcv2): Does this regex need to be more sophisticated?
        static PATH_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r#"/service/(?P<service>\w+)/operation/(?P<operation>\w+)"#).unwrap());

        &PATH_REGEX
    }
}

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

impl<S: Clone, B> Router<B> for RpcV2Router<S> {
    type Service = S;

    // TODO(rpcv2): Implement a proper error type
    type Error = Error;

    fn match_route(&self, request: &http::Request<B>) -> Result<Self::Service, Self::Error> {
        let request_path = request.uri().path();
        let regex = Self::path_regex();

        let captures = regex.captures(request_path).ok_or(Error::NotFound)?;
        let (service, operation) = (&captures["service"], &captures["operation"]);

        let route_key = format!("{service}.{operation}");

        let value = self.routes.get(route_key.as_str()).ok_or(Error::NotFound)?;

        Ok(value.clone())
    }
}

impl<S> FromIterator<(&'static str, S)> for RpcV2Router<S> {
    fn from_iter<T: IntoIterator<Item = (&'static str, S)>>(iter: T) -> Self {
        Self {
            routes: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use crate::proto::test_helpers::req;

    use super::{Router, RpcV2Router};

    #[test]
    fn path_regex_works() {
        let regex = RpcV2Router::<()>::path_regex();

        let captures = regex.captures("/service/MyService/operation/MyOperation").unwrap();
        assert_eq!("MyService", &captures["service"]);
        assert_eq!("MyOperation", &captures["operation"]);
    }

    #[tokio::test]
    async fn simple_routing() {
        let routes = vec!["Service.Operation"];
        let router: RpcV2Router<_> = routes.clone().into_iter().map(|op| (op, ())).collect();

        // Request should match
        let res = router.match_route(&req(&Method::GET, "/service/Service/operation/Operation", None));
        assert!(res.is_ok());
    }
}
