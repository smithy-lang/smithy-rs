/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod region {
    use crate::meta::region::{ProvideRegion, ProviderChain};

    use crate::profile;
    pub fn default_provider() -> impl ProvideRegion {
        ProviderChain::first_try(crate::environment::region::Provider::new())
            .or_else(profile::region::Provider::new())
    }
}

pub mod credential;
