/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[derive(Debug, Clone, Copy)]
pub struct RequestAttempts {
    attempts: u32,
}

impl RequestAttempts {
    #[cfg(any(feature = "test-util", test))]
    pub fn new(attempts: u32) -> Self {
        Self { attempts }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

impl From<u32> for RequestAttempts {
    fn from(attempts: u32) -> Self {
        Self { attempts }
    }
}
