/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Re-exports for types since moved to the aws-runtime crate.

/// Use aws_runtime::profile::profile_file::ProfileFiles instead.
#[deprecated(
    since = "1.1.10",
    note = "Use aws_runtime::profile::profile_file::ProfileFiles instead."
)]
pub type ProfileFiles = aws_runtime::profile::profile_file::ProfileFiles;

/// Use aws_runtime::profile::profile_file::Builder instead.
#[deprecated(
    since = "1.1.10",
    note = "Use aws_runtime::profile::profile_file::Builder."
)]
pub type Builder = aws_runtime::profile::profile_file::Builder;

/// Use aws_runtime::profile::profile_file::ProfileFileKind instead.
#[deprecated(
    since = "1.1.10",
    note = "Use aws_runtime::profile::profile_file::ProfileFileKind."
)]
pub type ProfileFileKind = aws_runtime::profile::profile_file::ProfileFileKind;
