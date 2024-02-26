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
    deferred_signer_settings: Option<event_stream::DeferredSignerSettings<'a>>,
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
    /// Sets the [`DeferredSignerSettings`](event_stream::DeferredSignerSettings) for the event stream signer (optional).
    pub fn deferred_signer_settings(
        mut self,
        deferred_signer_settings: event_stream::DeferredSignerSettings<'a>,
    ) -> Self {
        self.set_deferred_signer_settings(Some(deferred_signer_settings));
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`DeferredSignerSettings`](event_stream::DeferredSignerSettings) for the event stream signer (optional).
    pub fn set_deferred_signer_settings(
        &mut self,
        deferred_signer_settings: Option<event_stream::DeferredSignerSettings<'a>>,
    ) -> &mut Self {
        self.deferred_signer_settings = deferred_signer_settings;
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
            if let Some(deferred_signer_settings) = self.deferred_signer_settings {
                if let Some(signer_sender) = deferred_signer_settings
                    .config_bag()
                    .load::<aws_smithy_eventstream::frame::DeferredSignerSender>(
                ) {
                    let identity = deferred_signer_settings.identity();
                    let region = signing_params.name().to_owned();
                    let name = signing_params.name().to_owned();
                    let time_source = deferred_signer_settings.time_source();
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
        }
        auth::apply_signing_instructions(signing_instructions, request)?;
        Ok(())
    }
}

/// Event stream related settings for [`SigningApplication`]
#[cfg(feature = "event-stream")]
pub mod event_stream {
    use aws_smithy_async::time::SharedTimeSource;
    use aws_smithy_runtime_api::client::identity::Identity;
    use aws_smithy_types::config_bag::ConfigBag;

    /// Settings for the event stream signer to defer signing
    #[derive(Debug)]
    pub struct DeferredSignerSettings<'a> {
        config_bag: &'a ConfigBag,
        identity: &'a Identity,
        time_source: SharedTimeSource,
    }

    impl<'a> DeferredSignerSettings<'a> {
        /// Creates builder for [`DeferredSignerSettings`].
        pub fn builder() -> deferred_signer::Builder<'a> {
            deferred_signer::Builder::default()
        }

        pub(crate) fn config_bag(&self) -> &ConfigBag {
            &self.config_bag
        }

        pub(crate) fn identity(&self) -> &Identity {
            &self.identity
        }

        pub(crate) fn time_source(&self) -> SharedTimeSource {
            self.time_source.clone()
        }
    }

    /// Builder for creating [`DeferredSignerSettings`]
    pub mod deferred_signer {
        use crate::auth::signing_application::event_stream::DeferredSignerSettings;
        use aws_smithy_async::time::SharedTimeSource;
        use aws_smithy_runtime_api::box_error::BoxError;
        use aws_smithy_runtime_api::client::identity::Identity;
        use aws_smithy_types::config_bag::ConfigBag;

        /// Builder for [`DeferredSignerSettings`]
        #[derive(Debug, Default)]
        pub struct Builder<'a> {
            config_bag: Option<&'a ConfigBag>,
            identity: Option<&'a Identity>,
            time_source: Option<SharedTimeSource>,
        }

        impl<'a> Builder<'a> {
            /// Sets the [`ConfigBag`] for this builder (required).
            pub fn config_bag(mut self, config_bag: &'a ConfigBag) -> Self {
                self.set_config_bag(Some(config_bag));
                self
            }

            /// Sets the [`ConfigBag`] for this builder (required).
            pub fn set_config_bag(&mut self, config_bag: Option<&'a ConfigBag>) -> &mut Self {
                self.config_bag = config_bag;
                self
            }

            /// Sets the [`Identity`] for this builder (required).
            pub fn identity(mut self, identity: &'a Identity) -> Self {
                self.set_identity(Some(identity));
                self
            }

            /// Sets the [`Identity`] for this builder (required).
            pub fn set_identity(&mut self, identity: Option<&'a Identity>) -> &mut Self {
                self.identity = identity;
                self
            }

            /// Sets the [`SharedTimeSource`] this builder (optional).
            pub fn time_source(mut self, time_source: SharedTimeSource) -> Self {
                self.set_time_source(Some(time_source));
                self
            }

            /// Sets the [`SharedTimeSource`] this builder (optional).
            pub fn set_time_source(&mut self, time_source: Option<SharedTimeSource>) -> &mut Self {
                self.time_source = time_source;
                self
            }

            /// Builds a [`DeferredSignerSettings`] if all required fields are provided, otherwise
            /// returns a [`BoxError`].
            pub fn build(self) -> Result<DeferredSignerSettings<'a>, BoxError> {
                Ok(DeferredSignerSettings {
                    config_bag: self
                        .config_bag
                        .ok_or("missing required field `config_bag`")?,
                    identity: self.identity.ok_or("missing required field `identity`")?,
                    time_source: self.time_source.unwrap_or_default(),
                })
            }
        }
    }
}
