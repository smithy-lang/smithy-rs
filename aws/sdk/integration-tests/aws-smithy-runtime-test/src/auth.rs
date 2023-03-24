/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::orchestrator::BoxError;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;

#[derive(Debug)]
pub struct GetObjectAuthOrc {}

impl GetObjectAuthOrc {
    pub fn new() -> Self {
        Self {}
    }
}

impl RuntimePlugin for GetObjectAuthOrc {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        // TODO(orchestrator) put an auth orchestrator in the bag
        Ok(())
    }
}
