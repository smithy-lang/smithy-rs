/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// The target part size for an upload or download request.
#[derive(Debug, Clone)]
pub enum TargetPartSize {
    /// Automatically configure an optimal target part size based on the execution environment.
    Auto,

    /// Explicitly configured part size.
    Explicit(u64),
}

/// The concurrency settings to use for a single upload or download request.
#[derive(Debug, Clone)]
pub enum ConcurrencySetting {
    /// Automatically configure an optimal concurrency setting based on the execution environment.
    Auto,

    /// Explicitly configured concurrency setting.
    Explicit(usize),
}
