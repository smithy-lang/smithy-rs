/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_client::conns::Https;
use aws_smithy_client::hyper_ext::Adapter;
use aws_smithy_http::body::SdkBody;
use aws_smithy_orchestrator::{BoxFallibleFut, ConfigBag, Connection};

pub struct HyperConnection {
    adapter: Adapter<Https>,
}

impl HyperConnection {
    pub fn new() -> Self {
        Self {
            adapter: Adapter::builder().build(aws_smithy_client::conns::https()),
        }
    }
}

impl Connection<http::Request<SdkBody>, http::Response<SdkBody>> for HyperConnection {
    fn call(
        &self,
        req: &mut http::Request<SdkBody>,
        cfg: &ConfigBag,
    ) -> BoxFallibleFut<http::Response<SdkBody>> {
        todo!("hyper's connector wants to take ownership of req");
        // self.adapter.call(req)
    }
}
