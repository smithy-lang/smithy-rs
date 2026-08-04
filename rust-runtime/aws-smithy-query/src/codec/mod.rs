/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-serde codec for the awsQuery protocol.
//!
//! Serializes to URL-encoded form bodies (`Action=X&Version=Y&Param=value`).
//! Deserialization is handled by the XML codec in `aws-smithy-xml`.

pub mod serializer;
pub use serializer::QueryShapeSerializer;
