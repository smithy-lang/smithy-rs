/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::auth;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningParams};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;

/// Trait signing an HTTP request with a signature
///
/// # Examples
/// ```rust,no_run
/// use aws_runtime::sign::{SigningPackage, SignWith};
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
    /// Sign `self` with a [`SigningPackage`]
    fn sign_with(&mut self, package: &SigningPackage<'_>) -> Result<(), BoxError>;
}

impl SignWith for HttpRequest {
    fn sign_with(&mut self, package: &SigningPackage<'_>) -> Result<(), BoxError> {
        let (signing_instructions, _signature) = {
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
        }
        .into_parts();

        // If this is an event stream operation, set up the event stream signer
        #[cfg(feature = "event-stream")]
        {
            if let Some(deferred_signer_settings) = &package.deferred_signer_settings {
                if let Some(signer_sender) = deferred_signer_settings
                    .config_bag
                    .load::<aws_smithy_eventstream::frame::DeferredSignerSender>(
                ) {
                    let region = package
                        .signing_params
                        .region()
                        .expect("signing params should be for SigV4")
                        .to_owned();
                    let name = package.signing_params.name().to_owned();
                    let time_source = deferred_signer_settings.time_source.clone();
                    signer_sender
                        .send(
                            Box::new(crate::auth::sigv4::event_stream::SigV4MessageSigner::new(
                                _signature,
                                deferred_signer_settings.identity.clone(),
                                aws_types::region::SigningRegion::from(region),
                                aws_types::SigningName::from(name),
                                time_source,
                            )) as _,
                        )
                        .expect("failed to send deferred signer");
                }
            }
        }
        auth::apply_signing_instructions(signing_instructions, self)?;
        Ok(())
    }
}

/// A collection of fields required for the signing process
#[derive(Debug)]
pub struct SigningPackage<'a> {
    signing_params: &'a SigningParams<'a>,
    payload_override: Option<SignableBody<'a>>,
    #[cfg(feature = "event-stream")]
    deferred_signer_settings: Option<event_stream::DeferredSignerSettings<'a>>,
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
    #[cfg(feature = "event-stream")]
    deferred_signer_settings: Option<event_stream::DeferredSignerSettings<'a>>,
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

    #[cfg(feature = "event-stream")]
    #[allow(unused)]
    /// Sets the [`DeferredSignerSettings`](event_stream::DeferredSignerSettings) for the event stream signer (optional)
    pub(crate) fn deferred_signer_settings(
        mut self,
        deferred_signer_settings: event_stream::DeferredSignerSettings<'a>,
    ) -> Self {
        self.set_deferred_signer_settings(Some(deferred_signer_settings));
        self
    }

    #[cfg(feature = "event-stream")]
    /// Sets the [`DeferredSignerSettings`](event_stream::DeferredSignerSettings) for the event stream signer (optional)
    pub(crate) fn set_deferred_signer_settings(
        &mut self,
        deferred_signer_settings: Option<event_stream::DeferredSignerSettings<'a>>,
    ) -> &mut Self {
        self.deferred_signer_settings = deferred_signer_settings;
        self
    }

    /// Builds a [`SigningPackage`] if all required fields are provided, otherwise returns a [`BoxError`].
    pub fn build(self) -> Result<SigningPackage<'a>, BoxError> {
        Ok(SigningPackage {
            signing_params: self
                .signing_params
                .ok_or("missing required field `signing_params`")?,
            payload_override: self.payload_override,
            #[cfg(feature = "event-stream")]
            deferred_signer_settings: self.deferred_signer_settings,
        })
    }
}

#[cfg(feature = "event-stream")]
pub(crate) mod event_stream {
    use aws_smithy_async::time::SharedTimeSource;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::identity::Identity;
    use aws_smithy_types::config_bag::ConfigBag;

    /// Settings for the event stream signer to defer signing
    #[derive(Debug)]
    pub(crate) struct DeferredSignerSettings<'a> {
        pub(super) config_bag: &'a ConfigBag,
        pub(super) identity: &'a Identity,
        pub(super) time_source: SharedTimeSource,
    }

    impl<'a> DeferredSignerSettings<'a> {
        /// Creates builder for [`DeferredSignerSettings`]
        pub(crate) fn builder() -> Builder<'a> {
            Builder::default()
        }
    }

    /// Builder for [`DeferredSignerSettings`]
    #[derive(Debug, Default)]
    pub(crate) struct Builder<'a> {
        config_bag: Option<&'a ConfigBag>,
        identity: Option<&'a Identity>,
        time_source: Option<SharedTimeSource>,
    }

    impl<'a> Builder<'a> {
        /// Sets the [`ConfigBag`] for this builder (required)
        pub(crate) fn config_bag(mut self, config_bag: &'a ConfigBag) -> Self {
            self.set_config_bag(Some(config_bag));
            self
        }

        /// Sets the [`ConfigBag`] for this builder (required)
        pub(crate) fn set_config_bag(&mut self, config_bag: Option<&'a ConfigBag>) -> &mut Self {
            self.config_bag = config_bag;
            self
        }

        /// Sets the [`Identity`] for this builder (required)
        pub(crate) fn identity(mut self, identity: &'a Identity) -> Self {
            self.set_identity(Some(identity));
            self
        }

        /// Sets the [`Identity`] for this builder (required)
        pub(crate) fn set_identity(&mut self, identity: Option<&'a Identity>) -> &mut Self {
            self.identity = identity;
            self
        }

        /// Sets the [`SharedTimeSource`] this builder (optional)
        pub(crate) fn time_source(mut self, time_source: SharedTimeSource) -> Self {
            self.set_time_source(Some(time_source));
            self
        }

        /// Sets the [`SharedTimeSource`] this builder (optional)
        pub(crate) fn set_time_source(
            &mut self,
            time_source: Option<SharedTimeSource>,
        ) -> &mut Self {
            self.time_source = time_source;
            self
        }

        /// Builds a [`DeferredSignerSettings`] if all required fields are provided, otherwise
        /// returns a [`BoxError`].
        pub(crate) fn build(self) -> Result<DeferredSignerSettings<'a>, BoxError> {
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
