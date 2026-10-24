/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed service configuration shared by generated and schema-based implementations.

mod config;
mod request_body;

pub use config::ServiceConfig;
pub use request_body::{
    EventStreamTimeoutConfig, RequestBodyCollectionConfig, ServiceEventStreamConfig, ServiceRequestBodyConfig,
};
