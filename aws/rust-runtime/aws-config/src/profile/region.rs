/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::meta::region::{future, ProvideRegion};
use aws_types::os_shim_internal::{Env, Fs};
use aws_types::region::Region;

pub struct ProfileFileRegionProvider {
    fs: Fs,
    env: Env,
    profile_override: Option<String>,
}

impl ProfileFileRegionProvider {
    pub fn new() -> Self {
        Self {
            fs: Fs::real(),
            env: Env::real(),
            profile_override: None,
        }
    }

    async fn region(&self) -> Option<Region> {
        let profile = super::parser::load(&self.fs, &self.env)
            .await
            .map_err(|err| tracing::warn!(err = %err, "failed to parse profile"))
            .ok()?;
        let selected_profile = self
            .profile_override
            .as_deref()
            .unwrap_or_else(|| profile.selected_profile());
        let selected_profile = profile.get_profile(selected_profile)?;
        selected_profile
            .get("region")
            .map(|region| Region::new(region.to_owned()))
    }
}

impl ProvideRegion for ProfileFileRegionProvider {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::new(self.region())
    }
}
