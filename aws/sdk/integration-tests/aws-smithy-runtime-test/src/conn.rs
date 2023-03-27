/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_client::conns::Https;
use aws_smithy_client::hyper_ext::Adapter;
use aws_smithy_http::body::SdkBody;
use aws_smithy_runtime_api::client::orchestrator::{
    BoxError, BoxFallibleFut, Connection, HttpRequest,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;

#[derive(Debug)]
pub struct HyperConnection {
    _adapter: Adapter<Https>,
}

impl RuntimePlugin for HyperConnection {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        // TODO(orchestrator) put a connection in the bag
        Ok(())
    }
}

impl HyperConnection {
    pub fn new() -> Self {
        Self {
            _adapter: Adapter::builder().build(aws_smithy_client::conns::https()),
        }
    }
}

impl Connection for HyperConnection {
    fn call(
        &self,
        _req: &mut HttpRequest,
        _cfg: &ConfigBag,
    ) -> BoxFallibleFut<http::Response<SdkBody>> {
        todo!("hyper's connector wants to take ownership of req");
        // self.adapter.call(req)
    }
}
