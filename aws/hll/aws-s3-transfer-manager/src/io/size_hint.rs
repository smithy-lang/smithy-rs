/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// A body size hint
#[derive(Debug, Clone, Default)]
pub struct SizeHint {
    lower: u64,
    upper: Option<u64>,
}

impl SizeHint {
    /// Set an exact size hint with upper and lower set to `size` bytes.
    pub fn exact(size: u64) -> Self {
        Self {
            lower: size,
            upper: Some(size),
        }
    }

    /// Set the lower bound on the body size
    pub fn with_lower(self, lower: u64) -> Self {
        Self { lower, ..self }
    }

    /// Set the upper bound on the body size
    pub fn with_upper(self, upper: Option<u64>) -> Self {
        Self { upper, ..self }
    }

    /// Get the lower bound of the body size
    pub fn lower(&self) -> u64 {
        self.lower
    }

    /// Get the upper bound of the body size if known.
    pub fn upper(&self) -> Option<u64> {
        self.upper
    }
}
