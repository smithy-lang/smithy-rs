/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Carries no code. This crate exists for its dependency declarations: CI resolves its lockfile
//! and asserts that a consumer taking the SDK with default features gets no `http` 0.2.x.
