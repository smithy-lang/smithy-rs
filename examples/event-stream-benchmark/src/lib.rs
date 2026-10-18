/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Generated server event streams exercised entirely in process.
mod fixtures;
mod services;

use aws_smithy_http_server::body::{Body, BoxBody};
use http::{Request, Response};
use http_body_util::BodyExt;
use std::convert::Infallible;
use tower::{util::BoxCloneService, ServiceExt};

pub use fixtures::Fixture;

pub type Server = BoxCloneService<Request<Body>, Response<BoxBody>, Infallible>;
pub const METADATA: &str = "event-stream-benchmark-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    RestJson,
    RpcV2Cbor,
}

impl Protocol {
    pub const ALL: [Self; 2] = [Self::RestJson, Self::RpcV2Cbor];

    pub fn name(self) -> &'static str {
        match self {
            Self::RestJson => "rest-json",
            Self::RpcV2Cbor => "rpc-v2-cbor",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Upload,
    Download,
    Echo,
}

impl Direction {
    pub const ALL: [Self; 3] = [Self::Upload, Self::Download, Self::Echo];

    pub fn name(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Echo => "echo",
        }
    }

    pub fn operation(self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
            Self::Echo => "Echo",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Structured,
    Binary,
}

impl Kind {
    pub const ALL: [Self; 2] = [Self::Structured, Self::Binary];

    pub fn name(self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::Binary => "binary",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub protocol: Protocol,
    pub direction: Direction,
    pub kind: Kind,
    pub size: usize,
    pub count: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Implementation {
    Legacy,
    Schema,
    MultiSchema,
}

impl Implementation {
    pub const ALL: [Self; 3] = [Self::Legacy, Self::Schema, Self::MultiSchema];

    pub fn name(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Schema => "schema",
            Self::MultiSchema => "multi-schema",
        }
    }
}

pub fn server(implementation: Implementation, case: Case) -> Server {
    match (implementation, case.protocol) {
        (Implementation::Legacy, Protocol::RestJson) => services::rest_json_legacy::server(case),
        (Implementation::Schema, Protocol::RestJson) => services::rest_json_schema::server(case),
        (Implementation::Legacy, Protocol::RpcV2Cbor) => services::cbor_legacy::server(case),
        (Implementation::Schema, Protocol::RpcV2Cbor) => services::cbor_schema::server(case),
        (Implementation::MultiSchema, _) => services::multi_schema::server(case),
    }
}

#[derive(Debug)]
pub struct Drain {
    pub frames: usize,
    pub bytes: usize,
}

/// Polls every response frame. Returning only the response would skip lazy event serialization.
/// This does not decode output frames or allocate a whole-response buffer.
pub async fn drain(server: Server, request: Request<Body>) -> Drain {
    let response = server.oneshot(request).await.expect("infallible service");
    assert_eq!(response.status(), 200, "unexpected server rejection");
    let mut body = response.into_body();
    let mut result = Drain {
        frames: 0,
        bytes: 0,
    };
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.expect("response body").into_data() {
            result.frames += 1;
            result.bytes += data.len();
            std::hint::black_box(data);
        }
    }
    result
}
