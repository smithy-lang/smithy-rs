/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Module with client connectors useful for testing.
//!
//! Each test connector is useful for different test use cases:
//! - [`capture_request`](capture_request::capture_request): If you don't care what the
//! response is, but just want to check that the serialized request is what you expect,
//! then use `capture_request`. Or, alternatively, if you don't care what the request
//! is, but want to always respond with a given response, then capture request can also
//! be useful since you can optionally give it a response to return.
//! - [`dvr`]: If you want to record real-world traffic and then replay it later, then DVR's
//! [`RecordingConnector`](dvr::RecordingConnector) and [`ReplayingConnector`](dvr::ReplayingConnector)
//! can accomplish this, and the recorded traffic can be saved to JSON and checked in. Note: if
//! the traffic recording has sensitive information in it, such as signatures or authorization,
//! you will need to manually scrub this out if you intend to store the recording alongside
//! your tests.
//! - [`EventConnector`]: If you want to have a set list of requests and their responses in a test,
//! then the event connector will be useful. On construction, it takes a list of tuples that represent
//! each expected request and the response for that request. At the end of the test, you can ask the
//! connector to verify that the requests matched the expectations.
//! - [`infallible_connection_fn`]: Allows you to create a connector from an infallible function
//! that takes a request and returns a response.
//! - [`NeverConnector`]: Useful for testing timeouts, where you want the connector to never respond.

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
