/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(feature = "metrics")]

use aws_smithy_observability::{global::get_telemetry_provider, AttributeValue, Attributes};
use std::time::SystemTime;

#[derive(Clone, Debug, Default)]
pub(crate) struct OperationInfo {
    pub(crate) service_name: String,
    pub(crate) operation_name: String,
}

impl From<OperationInfo> for Attributes {
    fn from(value: OperationInfo) -> Self {
        let mut attrs = Attributes::new();
        attrs.set("rpc.service", AttributeValue::String(value.service_name));
        attrs.set("rpc.method", AttributeValue::String(value.operation_name));
        attrs
    }
}

/// Records the duration of the entire call to an operation
pub(crate) fn record_call_duration(start: Option<SystemTime>, info: OperationInfo) {
    let elapsed = start.map(|s| s.elapsed());
    let tp = get_telemetry_provider();
    let attrs = Attributes::from(info);
    if let (Some(Ok(dur)), Ok(tp)) = (elapsed, tp) {
        tp.meter_provider()
            .get_meter("orchestrator", None)
            .create_histogram("smithy.client.call.duration".into(), Some("s".into()), Some("Overall call duration (including retries and time to send or receive request and response body)".into()))
            .record(dur.as_secs_f64(), Some(&attrs), None);
    }
}

/// Records the duration of a single attempt to call to an operation
pub(crate) fn record_attempt_duration(
    start: Option<SystemTime>,
    attempt: u32,
    info: OperationInfo,
) {
    let elapsed = start.map(|s| s.elapsed());
    let tp = get_telemetry_provider();
    let mut attrs = Attributes::from(info);
    attrs.set("Attempt", AttributeValue::I64(attempt.into()));

    if let (Some(Ok(dur)), Ok(tp)) = (elapsed, tp) {
        tp.meter_provider()
            .get_meter("orchestrator", None)
            .create_histogram("smithy.client.call.attempt.duration".into(), Some("s".into()), Some("The time it takes to connect to the service, send the request, and get back HTTP status code and headers (including time queued waiting to be sent)".into()))
            .record(dur.as_secs_f64(), Some(&attrs), None);
    }
}
