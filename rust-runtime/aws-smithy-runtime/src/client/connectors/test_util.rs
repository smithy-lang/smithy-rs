/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Module with client connectors useful for testing.

mod capture_request;
pub use capture_request::{capture_request, CaptureRequestHandler, CaptureRequestReceiver};

#[cfg(feature = "connector-hyper")]
pub mod dvr;

mod event_connector;
pub use event_connector::{ConnectionEvent, EventConnector};

mod infallible;
pub use infallible::infallible_connection_fn;

mod never;
pub use never::NeverConnector;
