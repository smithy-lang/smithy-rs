/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod aws_json;
pub mod aws_json_10;
pub mod aws_json_11;
pub mod rest;
pub mod rest_json_1;
pub mod rest_xml;

#[cfg(test)]
pub mod test_helpers {
    use http::{HeaderMap, Method, Request};

    /// Helper function to build a `Request`. Used in other test modules.
    pub fn req(method: &Method, uri: &str, headers: Option<HeaderMap>) -> Request<()> {
        let mut r = Request::builder().method(method).uri(uri).body(()).unwrap();
        if let Some(headers) = headers {
            *r.headers_mut() = headers
        }
        r
    }

    // Returns a `Response`'s body as a `String`, without consuming the response.
    pub async fn get_body_as_string<B>(body: B) -> String
    where
        B: http_body::Body + std::marker::Unpin,
        B::Error: std::fmt::Debug,
    {
        let body_bytes = hyper::body::to_bytes(body).await.unwrap();
        String::from(std::str::from_utf8(&body_bytes).unwrap())
    }
}
