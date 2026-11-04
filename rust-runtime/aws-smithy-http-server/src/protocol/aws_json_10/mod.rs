/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod router;
/// The `awsJson1_0` protocol.
///
/// The same type is used as the protocol type parameter of generated serde servers and, with
/// `codegen.schemaSerde`, as the schema-driven protocol implementation.
pub use crate::schema::protocol::AwsJson1_0;
