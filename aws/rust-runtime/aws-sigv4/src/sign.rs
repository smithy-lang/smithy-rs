/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types for creating signing keys, calculating signatures, and applying them to HTTP requests.

use crate::http_request::{
    sign, SignableBody, SignableRequest, SigningInstructions, SigningParams,
};
use crate::SigningOutput;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;

/// Support for Sigv4 signing
pub mod v4;

/// Support for Sigv4a signing
#[cfg(feature = "sigv4a")]
pub mod v4a;

/// Trait signing an HTTP request with a signature
///
/// # Examples
/// ```rust,no_run
/// use aws_sigv4::http_request::{SigningInstructions, SigningParams};
/// use aws_sigv4::sign::{SigningPackage, SignWith};
/// use aws_sigv4::SigningOutput;
/// # use aws_smithy_runtime_api::box_error::BoxError;
/// use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
///
/// fn signing_params<'a>() -> SigningParams<'a> {
///     // --snip--
/// #    todo!()
/// }
///
/// fn request() -> HttpRequest {
///     // --snip--
/// #   HttpRequest::empty()
/// }
///
/// # fn example() -> Result<SigningOutput<SigningInstructions>, BoxError> {
/// let signing_params = signing_params();
/// let signing_package = SigningPackage::builder()
///     .signing_params(&signing_params)
///     .build()
///     .expect("required fields have been set");
///
/// let mut request = request();
/// request.sign_with(&signing_package)
/// # }
/// ```
pub trait SignWith {
    /// Sign `self` with a [`SigningPackage`] and return the [`SigningOutput<SigningInstructions>`](SigningOutput)
    /// that was used during the signing process.
    ///
    /// Most of the time, the return value can be ignored since it has already been applied to the
    /// given request, but there are cases where calling code needs to have access to it even after
    /// signing is done (e.g. deferring signing for event stream).
    fn sign_with(
        &mut self,
        package: &SigningPackage<'_>,
    ) -> Result<SigningOutput<SigningInstructions>, BoxError>;
}

impl SignWith for HttpRequest {
    fn sign_with(
        &mut self,
        package: &SigningPackage<'_>,
    ) -> Result<SigningOutput<SigningInstructions>, BoxError> {
        let signing_output = {
            // A body that is already in memory can be signed directly. A body that is not in memory
            // (any sort of streaming body or presigned request) will be signed via UNSIGNED-PAYLOAD.
            let signable_body = package
                .payload_override
                .as_ref()
                // the payload_override is a cheap clone because it contains either a
                // reference or a short checksum (we're not cloning the entire body)
                .cloned()
                .unwrap_or_else(|| {
                    self.body()
                        .bytes()
                        .map(SignableBody::Bytes)
                        .unwrap_or(SignableBody::UnsignedPayload)
                });

            let signable_request = SignableRequest::new(
                self.method(),
                self.uri(),
                self.headers().iter(),
                signable_body,
            )?;
            sign(signable_request, package.signing_params)?
        };

        apply_signing_instructions(&signing_output.output, self)?;

        Ok(signing_output)
    }
}

/// A collection of fields required for the signing process
#[derive(Debug)]
pub struct SigningPackage<'a> {
    signing_params: &'a SigningParams<'a>,
    payload_override: Option<SignableBody<'a>>,
}

impl<'a> SigningPackage<'a> {
    /// Returns a default Builder
    pub fn builder() -> Builder<'a> {
        Builder::default()
    }
}

/// Builder for [`SigningPackage`]
#[derive(Debug, Default)]
pub struct Builder<'a> {
    signing_params: Option<&'a SigningParams<'a>>,
    payload_override: Option<SignableBody<'a>>,
}

impl<'a> Builder<'a> {
    /// Sets the [`SigningParams`] for the builder (required)
    pub fn signing_params(mut self, signing_params: &'a SigningParams<'_>) -> Self {
        self.set_signing_params(Some(signing_params));
        self
    }

    /// Sets the [`SigningParams`] for the builder (required)
    pub fn set_signing_params(
        &mut self,
        signing_params: Option<&'a SigningParams<'_>>,
    ) -> &mut Self {
        self.signing_params = signing_params;
        self
    }

    /// Sets the [`SignableBody`] for the builder (optional)
    pub fn payload_override(mut self, payload_override: SignableBody<'a>) -> Self {
        self.set_payload_override(Some(payload_override));
        self
    }

    /// Sets the [`SignableBody`] for the builder (optional)
    pub fn set_payload_override(
        &mut self,
        payload_override: Option<SignableBody<'a>>,
    ) -> &mut Self {
        self.payload_override = payload_override;
        self
    }

    /// Builds a [`SigningPackage`] if all required fields are provided, otherwise returns a [`BoxError`].
    pub fn build(self) -> Result<SigningPackage<'a>, BoxError> {
        Ok(SigningPackage {
            signing_params: self
                .signing_params
                .ok_or("missing required field `signing_params`")?,
            payload_override: self.payload_override,
        })
    }
}

fn apply_signing_instructions(
    instructions: &SigningInstructions,
    request: &mut HttpRequest,
) -> Result<(), BoxError> {
    let (new_headers, new_query) = instructions.parts();
    for header in new_headers {
        let mut value = http0::HeaderValue::from_str(header.value()).unwrap();
        value.set_sensitive(header.sensitive());
        request.headers_mut().insert(header.name(), value);
    }

    if !new_query.is_empty() {
        let mut query = aws_smithy_http::query_writer::QueryWriter::new_from_string(request.uri())?;
        for (name, value) in new_query {
            query.insert(name, value);
        }
        request.set_uri(query.build_uri())?;
    }
    Ok(())
}
