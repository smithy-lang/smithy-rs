/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP specific retry behaviors
//!
//! For protocol agnostic retries, see `smithy_types::Retry`.

use smithy_types::retry::{ProvideErrorKind, RetryKind};

pub trait ClassifyResponse {
    fn classify<E, B>(&self, e: E, response: &http::Response<B>) -> RetryKind
    where
        E: ProvideErrorKind;
}
