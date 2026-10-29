/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod rejection;
pub mod router;
pub mod runtime_error;
/// The `restXml` protocol.
///
/// The same type is used as the protocol type parameter of generated serde servers and, with
/// `codegen.schemaSerde`, as the schema-driven protocol implementation.
pub use crate::schema::protocol::RestXml;
