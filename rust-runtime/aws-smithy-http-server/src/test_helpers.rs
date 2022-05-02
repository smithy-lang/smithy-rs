/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub(crate) fn assert_send<T: Send>() {}
pub(crate) fn assert_sync<T: Sync>() {}
// Returns a HTTP body as a `String`, without consuming the request or response.
pub(crate) async fn get_body_as_string<B>(body: &mut B) -> String
where
    B: http_body::Body + std::marker::Unpin,
    B::Error: std::fmt::Debug,
{
    let body_bytes = hyper::body::to_bytes(body).await.unwrap();
    String::from(std::str::from_utf8(&body_bytes).unwrap())
}
