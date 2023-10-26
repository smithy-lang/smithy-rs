/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// At least one body feature must be enabled to use this
#[cfg(any(feature = "http-body-0-4-x"))]
pub mod minimum_throughput;
