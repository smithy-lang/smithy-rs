/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default Provider chains for [`region`](default_provider::region) and credentials (TODO)

pub mod region {
    //! Default region provider chain

    use crate::environment::region::EnvironmentVariableRegionProvider;
    use crate::meta::region::ProvideRegion;

    /// Default Region Provider chain
    ///
    /// This provider will load region from environment variables.
    pub fn default_provider() -> impl ProvideRegion {
        EnvironmentVariableRegionProvider::new()
    }
}
