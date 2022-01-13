/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2021 Tower Contributors
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

//! Extension extraction to share state across handlers.

use super::rejection::{ExtensionHandlingRejection, ExtensionsAlreadyExtracted, MissingExtension};
use async_trait::async_trait;
use axum_core::extract::{FromRequest, RequestParts};
use std::ops::Deref;

/// Extension type used to store information in HTTP responses.
#[derive(Debug, Clone)]
pub struct ResponseExtensions {
    /// Smithy model namespace.
    namespace: &'static str,
    /// Smithy operation name.
    operation_name: &'static str,
}

impl ResponseExtensions {
    /// Creates a new `ResponseExtensions`.
    pub fn new(namespace: &'static str, operation_name: &'static str) -> Self {
        Self {
            namespace,
            operation_name,
        }
    }

    /// Returns the current operation formatted as `<namespace>#<operation_name>`.
    pub fn operation(&self) -> String {
        format!("{}#{}", self.namespace, self.operation_name)
    }
}

/// Extension type used to store the type of user defined error returned by an operation.
/// These are modeled errors, defined in the Smithy model.
#[derive(Debug, Clone)]
pub struct ExtensionModeledError(&'static str);
impl_extension_new_and_deref!(ExtensionModeledError);

/// Extension type used to store the type of framework error caught during execution.
/// These are unmodeled error, or rejection, defined in the runtime crates.
#[derive(Debug, Clone)]
pub struct ExtensionRejection(String);

impl ExtensionRejection {
    /// Returns a new `ExtensionRejection`.
    pub fn new(value: String) -> ExtensionRejection {
        ExtensionRejection(value)
    }
}

impl Deref for ExtensionRejection {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extractor that gets a value from [request extensions].
///
/// This is commonly used to share state across handlers.
///
/// If the extension is missing it will reject the request with a `500 Internal
/// Server Error` response.
///
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
#[derive(Debug, Clone, Copy)]
pub struct Extension<T>(pub T);

#[async_trait]
impl<T, B> FromRequest<B> for Extension<T>
where
    T: Clone + Send + Sync + 'static,
    B: Send,
{
    type Rejection = ExtensionHandlingRejection;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let value = req
            .extensions()
            .ok_or(ExtensionsAlreadyExtracted)?
            .get::<T>()
            .ok_or_else(|| {
                MissingExtension::from_err(format!(
                    "Extension of type `{}` was not found. Perhaps you forgot to add it?",
                    std::any::type_name::<T>()
                ))
            })
            .map(|x| x.clone())?;

        Ok(Extension(value))
    }
}

impl<T> Deref for Extension<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
