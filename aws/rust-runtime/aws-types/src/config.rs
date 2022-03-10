/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#![deny(missing_docs)]

//! AWS Shared Config (deprecated, replaced with [`aws_types::SdkConfig`](aws_types::SdkConfig))
//!
//! This module contains an shared configuration representation that is agnostic from a specific service.

#[deprecated(since = "0.9.0", note = "renamed to aws_types::SdkConfig")]
/// AWS Shared Configuration
pub type Config = super::SdkConfig;
