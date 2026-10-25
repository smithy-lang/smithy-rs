/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed service configuration shared by generated and schema-based implementations.

mod event_stream;
mod request_body;
mod service;

pub use event_stream::{EventStreamTimeoutConfig, ServiceEventStreamConfig};
pub use request_body::{RequestBodyCollectionConfig, ServiceRequestBodyConfig};
pub use service::ServiceConfig;
