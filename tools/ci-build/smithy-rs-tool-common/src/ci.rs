/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Returns `true` if this code is being run in CI
pub fn running_in_ci() -> bool {
    std::env::var("GITHUB_ACTIONS").unwrap_or_default() == "true"
        || std::env::var("SMITHY_RS_DOCKER_BUILD_IMAGE").unwrap_or_default() == "1"
}
