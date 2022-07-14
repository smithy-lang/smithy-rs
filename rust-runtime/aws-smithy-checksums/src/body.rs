/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body-wrappers that calculate and validate checksums.

mod calculate;
pub use calculate::ChecksumBody;

mod validate;
pub use validate::ChecksumValidatedBody;
