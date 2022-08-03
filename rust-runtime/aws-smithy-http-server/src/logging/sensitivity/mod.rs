/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The combination of [HTTP binding traits] and the [sensitive trait] require us to redact
//! portions of the HTTP requests/responses during logging.
//!
//! [HTTP binding traits]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html
//! [sensitive trait]: https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html?highlight=sensitive#sensitive-trait

pub mod headers;
mod request;
mod response;
mod sensitive;
pub mod uri;

pub use request::*;
pub use response::*;
pub use sensitive::*;

/// The string placeholder for redacted data.
pub const REDACTED: &str = "{redacted}";
