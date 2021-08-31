/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default Provider chains for [`region`](default_provider::region) and credentials (TODO)

/// Default region provider chain
pub mod region {

    use crate::environment::region::EnvironmentVariableRegionProvider;
    use crate::meta::region::ProvideRegion;

    /// Default Region Provider chain
    ///
    /// This provider will load region from environment variables.
    pub fn default_provider() -> impl ProvideRegion {
        EnvironmentVariableRegionProvider::new()
    }
}

/// Default credentials provider chain
pub mod credentials {
    use crate::environment::credentials::EnvironmentVariableCredentialsProvider;
    use aws_types::credentials::ProvideCredentials;

    /// Default Region Provider chain
    ///
    /// This provider will load region from environment variables.
    pub fn default_provider() -> impl ProvideCredentials {
        EnvironmentVariableCredentialsProvider::new()
    }
}
