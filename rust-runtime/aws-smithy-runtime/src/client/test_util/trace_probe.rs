/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::orchestrator::TraceProbe;

#[derive(Debug)]
pub struct NoOpTraceProbe {}

impl NoOpTraceProbe {
    pub fn new() -> Self {
        Self {}
    }
}

impl TraceProbe for NoOpTraceProbe {
    fn dispatch_events(&self) {
        // Do nothing
    }
}
