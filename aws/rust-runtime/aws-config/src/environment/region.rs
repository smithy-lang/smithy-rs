/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::meta::region::{future, ProvideRegion};
use aws_types::os_shim_internal::Env;
use aws_types::region::Region;

pub struct Provider {
    env: Env,
}

#[allow(clippy::redundant_closure)] // https://github.com/rust-lang/rust-clippy/issues/7218
impl Provider {
    pub fn new() -> Self {
        Provider { env: Env::real() }
    }
}

impl ProvideRegion for Provider {
    fn region(&self) -> future::ProvideRegion {
        let region = self
            .env
            .get("AWS_REGION")
            .or_else(|_| self.env.get("AWS_DEFAULT_REGION"))
            .map(Region::new)
            .ok();
        future::ProvideRegion::ready(region)
    }
}
