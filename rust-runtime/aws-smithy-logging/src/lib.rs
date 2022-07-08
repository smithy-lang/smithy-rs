/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod sensitive;

pub use sensitive::*;

/// The string placeholder for redacted data.
pub const REDACTED: &str = "{redacted}";
