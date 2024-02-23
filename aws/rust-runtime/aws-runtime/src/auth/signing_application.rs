/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::auth;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningParams};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;

/// A builder that configures fields for signing and, upon calling `.finalize()`, produces a
/// signature and adds signing headers to the given request
///
/// # Examples
/// ```rust,no_run
/// use aws_runtime::auth::signing_application::SigningApplication;
/// use aws_sigv4::http_request::SigningParams;
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
/// # fn example() -> Result<(), BoxError> {
/// let mut request = request();
/// let signing_params = signing_params();
/// SigningApplication::default()
///     .request(&mut request)
///     .signing_params(&signing_params)
///     .finalize()
/// # }
/// ```
#[derive(Debug, Default)]
pub struct SigningApplication<'a> {
    request: Option<&'a mut HttpRequest>,
    signing_params: Option<&'a SigningParams<'a>>,
    payload_override: Option<SignableBody<'a>>,
    #[cfg(feature = "event-stream")]
    signer_sender: Option<&'a aws_smithy_eventstream::frame::DeferredSignerSender>,
    #[cfg(feature = "event-stream")]
    identity: Option<&'a aws_smithy_runtime_api::client::identity::Identity>,
    #[cfg(feature = "event-stream")]
    time_source: Option<aws_smithy_async::time::SharedTimeSource>,
}

impl<'a> SigningApplication<'a> {
    /// Sets the request (required).
    pub fn request(mut self, request: &'a mut HttpRequest) -> Self {
        self.set_request(Some(request));
        self
    }

    /// Sets the request (required).
    pub fn set_request(&mut self, request: Option<&'a mut HttpRequest>) -> &mut Self {
        self.request = request;
        self
    }

    /// Sets the [`SigningParams`] for the builder (required).
    pub fn signing_params(mut self, signing_params: &'a SigningParams<'_>) -> Self {
        self.set_signing_params(Some(signing_params));
        self
    }

    /// Sets the [`SigningParams`] for the builder (required).
    pub fn set_signing_params(
        &mut self,
        signing_params: Option<&'a SigningParams<'_>>,
    ) -> &mut Self {
        self.signing_params = signing_params;
        self
    }

    /// Sets the [`SignableBody`] for the builder (optional).
    pub fn payload_override(mut self, payload_override: SignableBody<'a>) -> Self {
        self.set_payload_override(Some(payload_override));
        self
    }

    /// Sets the [`SignableBody`] for the builder (optional).
    pub fn set_payload_override(
        &mut self,
        payload_override: Option<SignableBody<'a>>,
    ) -> &mut Self {
        self.payload_override = payload_override;
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the sender for event the stream signer to defer signing (optional).
    pub fn signer_sender(
        mut self,
        signer_sender: &'a aws_smithy_eventstream::frame::DeferredSignerSender,
    ) -> Self {
        self.set_signer_sender(Some(signer_sender));
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the sender for event the stream signer to defer signing (optional).
    pub fn set_signer_sender(
        &mut self,
        signer_sender: Option<&'a aws_smithy_eventstream::frame::DeferredSignerSender>,
    ) -> &mut Self {
        self.signer_sender = signer_sender;
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`Identity`](aws_smithy_runtime_api::client::identity::Identity) used for the event
    /// stream signer (required if `self.signer_sender` is present).
    pub fn identity(
        mut self,
        identity: &'a aws_smithy_runtime_api::client::identity::Identity,
    ) -> Self {
        self.set_identity(Some(identity));
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`Identity`](aws_smithy_runtime_api::client::identity::Identity) used for the event
    /// stream signer (required if `self.signer_sender` is present).
    pub fn set_identity(
        &mut self,
        identity: Option<&'a aws_smithy_runtime_api::client::identity::Identity>,
    ) -> &mut Self {
        self.identity = identity;
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`SharedTimeSource`](aws_smithy_async::time::SharedTimeSource) used for the event stream signer
    /// (optional).
    pub fn time_source(mut self, time_source: aws_smithy_async::time::SharedTimeSource) -> Self {
        self.set_time_source(Some(time_source));
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`SharedTimeSource`](aws_smithy_async::time::SharedTimeSource) used for the event stream signer
    /// (optional).
    pub fn set_time_source(
        &mut self,
        time_source: Option<aws_smithy_async::time::SharedTimeSource>,
    ) -> &mut Self {
        self.time_source = time_source;
        self
    }

    /// Produces a signature for the given request and applies signing headers to `self.request`.
    pub fn finalize(self) -> Result<(), BoxError> {
        let request = self.request.ok_or("missing required field `request`")?;
        let signing_params = self
            .signing_params
            .ok_or("missing required field `signing_params`")?;

        let (signing_instructions, _signature) = {
            // A body that is already in memory can be signed directly. A body that is not in memory
            // (any sort of streaming body or presigned request) will be signed via UNSIGNED-PAYLOAD.
            let signable_body = self
                .payload_override
                .as_ref()
                // the payload_override is a cheap clone because it contains either a
                // reference or a short checksum (we're not cloning the entire body)
                .cloned()
                .unwrap_or_else(|| {
                    request
                        .body()
                        .bytes()
                        .map(SignableBody::Bytes)
                        .unwrap_or(SignableBody::UnsignedPayload)
                });

            let signable_request = SignableRequest::new(
                request.method(),
                request.uri(),
                request.headers().iter(),
                signable_body,
            )?;
            sign(signable_request, signing_params)?
        }
        .into_parts();

        // If this is an event stream operation, set up the event stream signer
        #[cfg(feature = "event-stream")]
        {
            if let Some(signer_sender) = self.signer_sender {
                let identity = self
                    .identity
                    .ok_or("missing required field for the event stream signer `identity`")?;
                let region = signing_params.name().to_owned();
                let name = signing_params.name().to_owned();
                let time_source = self.time_source.unwrap_or_default();
                signer_sender
                    .send(
                        Box::new(crate::auth::sigv4::event_stream::SigV4MessageSigner::new(
                            _signature,
                            identity.clone(),
                            aws_types::region::SigningRegion::from(region),
                            aws_types::SigningName::from(name),
                            time_source,
                        )) as _,
                    )
                    .expect("failed to send deferred signer");
            }
        }
        auth::apply_signing_instructions(signing_instructions, request)?;
        Ok(())
    }
}
