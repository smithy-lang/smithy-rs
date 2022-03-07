/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// Load app name from the environment
pub mod app_name;
pub use app_name::EnvironmentVariableAppNameProvider;

/// Load credentials from the environment
pub mod credentials;
pub use credentials::EnvironmentVariableCredentialsProvider;

/// Load regions from the environment
pub mod region;
pub use region::EnvironmentVariableRegionProvider;

/// Load retry behavior configuration from the environment
pub mod retry_config;
pub use retry_config::EnvironmentVariableRetryConfigProvider;

/// Load timeout configuration from the environment
pub mod shared_timeout_config;
pub use shared_timeout_config::EnvironmentVariableSharedTimeoutConfigProvider;
