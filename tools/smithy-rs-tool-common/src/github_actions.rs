/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// Returns `true` if this code is being run in GitHub Actions
pub fn running_in_github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").unwrap_or_default() == "true"
}
