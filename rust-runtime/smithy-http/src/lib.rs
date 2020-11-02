/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// TODO: there is no compelling reason to have this be a shared crate—we should vendor this
// module into the individual crates
pub mod base64;
pub mod label;
pub mod query;
