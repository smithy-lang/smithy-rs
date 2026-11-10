/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

//! Call Smithy services from a model loaded at runtime, without generated code.
//!
//! The dynamic client reads a Smithy model when the program runs, and calls the model's
//! operations by name. Inputs and outputs are documents shaped by the model.
