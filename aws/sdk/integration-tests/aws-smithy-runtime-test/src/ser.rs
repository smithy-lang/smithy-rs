/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime::{BoxError, HttpRequest, RequestSerializer};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::interceptors::context::Input;
use aws_smithy_runtime_api::runtime_plugin::RuntimePlugin;

#[derive(Debug)]
pub struct GetObjectInputSerializer {}

impl GetObjectInputSerializer {
    pub fn new() -> Self {
        Self {}
    }
}

impl RuntimePlugin for GetObjectInputSerializer {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        // TODO(orchestrator) put a serializer in the bag
        Ok(())
    }
}

impl RequestSerializer for GetObjectInputSerializer {
    fn serialize_input(&self, _input: &Input, _cfg: &ConfigBag) -> Result<HttpRequest, BoxError> {
        todo!()
    }
}
