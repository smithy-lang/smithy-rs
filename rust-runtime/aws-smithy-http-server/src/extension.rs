/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Extension types.
//!
//! Extension types are types that are stored in and extracted from _both_ requests and
//! responses.
//!
//! There is only one _generic_ extension type _for requests_, [`Extension`].
//!
//! On the other hand, the server SDK uses multiple concrete extension types for responses in order
//! to store a variety of information, like the operation that was executed, the operation error
//! that got returned, or the runtime error that happened, among others. The information stored in
//! these types may be useful to [`tower::Layer`]s that post-process the response: for instance, a
//! particular metrics layer implementation might want to emit metrics about the number of times an
//! an operation got executed.
//!
//! [extensions]: https://docs.rs/http/latest/http/struct.Extensions.html

use std::ops::Deref;

use thiserror::Error;

#[allow(deprecated)]
use crate::request::RequestParts;

pub use crate::request::extension::Extension;
pub use crate::request::extension::MissingExtension;

/// Extension type used to store information about Smithy operations in HTTP responses.
/// This extension type is set when it has been correctly determined that the request should be
/// routed to a particular operation. The operation handler might not even get invoked because the
/// request fails to deserialize into the modeled operation input.
///
/// The format given must be the absolute shape ID with `#` replaced with a `.`.
#[derive(Debug, Clone)]
#[deprecated(
    since = "0.52.0",
    note = "This is no longer inserted by the new service builder. Layers should be constructed per operation using the plugin system."
)]
pub struct OperationExtension {
    absolute: &'static str,

    namespace: &'static str,
    name: &'static str,
}

/// An error occurred when parsing an absolute operation shape ID.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    #[error(". was not found - missing namespace")]
    MissingNamespace,
}

#[allow(deprecated)]
impl OperationExtension {
    /// Creates a new [`OperationExtension`] from the absolute shape ID of the operation with `#` symbol replaced with a `.`.
    pub fn new(absolute_operation_id: &'static str) -> Result<Self, ParseError> {
        let (namespace, name) = absolute_operation_id
            .rsplit_once('.')
            .ok_or(ParseError::MissingNamespace)?;
        Ok(Self {
            absolute: absolute_operation_id,
            namespace,
            name,
        })
    }

    /// Returns the Smithy model namespace.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Returns the Smithy operation name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the absolute operation shape ID.
    pub fn absolute(&self) -> &'static str {
        self.absolute
    }
}

/// Extension type used to store the type of user-modeled error returned by an operation handler.
/// These are modeled errors, defined in the Smithy model.
#[derive(Debug, Clone)]
pub struct ModeledErrorExtension(&'static str);

impl ModeledErrorExtension {
    /// Creates a new `ModeledErrorExtension`.
    pub fn new(value: &'static str) -> ModeledErrorExtension {
        ModeledErrorExtension(value)
    }
}

impl Deref for ModeledErrorExtension {
    type Target = &'static str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extension type used to store the _name_ of the possible runtime errors.
/// These are _unmodeled_ errors; the operation handler was not invoked.
#[derive(Debug, Clone)]
pub struct RuntimeErrorExtension(String);

impl RuntimeErrorExtension {
    /// Creates a new `RuntimeErrorExtension`.
    pub fn new(value: String) -> RuntimeErrorExtension {
        RuntimeErrorExtension(value)
    }
}

impl Deref for RuntimeErrorExtension {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extract an [`Extension`] from a request.
/// This is essentially the implementation of `FromRequest` for `Extension`, but with a
/// protocol-agnostic rejection type. The actual code-generated implementation simply delegates to
/// this function and converts the rejection type into a [`crate::runtime_error::RuntimeError`].
#[deprecated(
    since = "0.52.0",
    note = "This was used for extraction under the older service builder. The `FromParts::from_parts` method is now used instead."
)]
#[allow(deprecated)]
pub async fn extract_extension<T, B>(
    req: &mut RequestParts<B>,
) -> Result<Extension<T>, crate::rejection::RequestExtensionNotFoundRejection>
where
    T: Clone + Send + Sync + 'static,
    B: Send,
{
    let value = req
        .extensions()
        .ok_or(crate::rejection::RequestExtensionNotFoundRejection::ExtensionsAlreadyExtracted)?
        .get::<T>()
        .ok_or_else(|| {
            crate::rejection::RequestExtensionNotFoundRejection::MissingExtension(format!(
                "Extension of type `{}` was not found. Perhaps you forgot to add it?",
                std::any::type_name::<T>()
            ))
        })
        .map(|x| x.clone())?;

    Ok(Extension(value))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn ext_accept() {
        let value = "com.amazonaws.ebs.CompleteSnapshot";
        let ext = OperationExtension::new(value).unwrap();

        assert_eq!(ext.absolute(), value);
        assert_eq!(ext.namespace(), "com.amazonaws.ebs");
        assert_eq!(ext.name(), "CompleteSnapshot");
    }

    #[test]
    fn ext_reject() {
        let value = "CompleteSnapshot";
        assert_eq!(
            OperationExtension::new(value).unwrap_err(),
            ParseError::MissingNamespace
        )
    }
}
