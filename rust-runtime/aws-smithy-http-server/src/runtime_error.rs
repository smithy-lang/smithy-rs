/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// A _protocol-agnostic_ type representing an internal framework error. As of writing, this can only
/// occur upon failure to extract an [`crate::extension::Extension`] from the request.
/// This type is converted into protocol-specific error variants. For example, in the
/// [`crate::protocol::rest_json_1`] protocol, it is converted to the
/// [`crate::protocol::rest_json_1::runtime_error::RuntimeError::InternalFailure`] variant.
pub struct InternalFailureException;

pub const INVALID_HTTP_RESPONSE_FOR_RUNTIME_ERROR_PANIC_MESSAGE: &str = "invalid HTTP response for `RuntimeError`; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues";
