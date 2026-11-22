/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors from building a dynamic client and from its calls.

mod build;
mod call;
mod operation;

pub use build::BuildError;
#[cfg(test)]
pub(crate) use build::BuildErrorKind;
pub(crate) use build::MissingHttpClient;
pub use call::{InputError, InputErrorKind, OperationNotFound};
pub use operation::DynamicOperationError;
