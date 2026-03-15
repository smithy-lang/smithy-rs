/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AWS REST JSON 1.0 protocol implementation.
//!
//! This module provides [`AwsRestJsonProtocol`], which constructs an
//! [`HttpBindingProtocol`] with a [`JsonCodec`] configured for the
//! `aws.protocols#restJson1` protocol:
//!
//! - Uses `@jsonName` trait for JSON property names
//! - Default timestamp format: `epoch-seconds`
//! - Content-Type: `application/json`

use crate::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_schema::http_protocol::HttpBindingProtocol;
use aws_smithy_schema::ShapeId;

static PROTOCOL_ID: ShapeId = ShapeId::from_static("aws.protocols", "restJson1", "");

/// AWS REST JSON 1.0 protocol (`aws.protocols#restJson1`).
///
/// This is a thin configuration wrapper that constructs an [`HttpBindingProtocol`]
/// with a [`JsonCodec`] using REST JSON settings. The `HttpBindingProtocol` handles
/// splitting members between HTTP bindings and the JSON payload.
pub struct AwsRestJsonProtocol {
    inner: HttpBindingProtocol<JsonCodec>,
}

impl AwsRestJsonProtocol {
    /// Creates a new REST JSON protocol with default settings.
    pub fn new() -> Self {
        let codec = JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(true)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .build(),
        );
        Self {
            inner: HttpBindingProtocol::new(PROTOCOL_ID, codec, "application/json"),
        }
    }

    /// Returns a reference to the inner `HttpBindingProtocol`.
    pub fn inner(&self) -> &HttpBindingProtocol<JsonCodec> {
        &self.inner
    }
}

impl Default for AwsRestJsonProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for AwsRestJsonProtocol {
    type Target = HttpBindingProtocol<JsonCodec>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
