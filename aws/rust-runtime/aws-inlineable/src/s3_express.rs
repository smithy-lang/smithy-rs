/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Supporting code for S3 Express auth
pub(crate) mod auth {
    use aws_runtime::auth::sigv4::SigV4Signer;
    use aws_sigv4::http_request::{SignatureLocation, SigningSettings};
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::auth::{
        AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, Sign,
    };
    use aws_smithy_runtime_api::client::identity::{Identity, SharedIdentityResolver};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::{
        GetIdentityResolver, RuntimeComponents,
    };
    use aws_smithy_types::config_bag::ConfigBag;

    /// Auth scheme ID for S3 Express.
    pub(crate) const SCHEME_ID: AuthSchemeId = AuthSchemeId::new("sigv4-s3express");

    /// S3 Express auth scheme.
    #[derive(Debug, Default)]
    pub(crate) struct S3ExpressAuthScheme {
        signer: S3ExpressSigner,
    }

    impl S3ExpressAuthScheme {
        /// Creates a new `S3ExpressAuthScheme`.
        pub(crate) fn new() -> Self {
            Default::default()
        }
    }

    impl AuthScheme for S3ExpressAuthScheme {
        fn scheme_id(&self) -> AuthSchemeId {
            SCHEME_ID
        }

        fn identity_resolver(
            &self,
            identity_resolvers: &dyn GetIdentityResolver,
        ) -> Option<SharedIdentityResolver> {
            identity_resolvers.identity_resolver(self.scheme_id())
        }

        fn signer(&self) -> &dyn Sign {
            &self.signer
        }
    }

    /// S3 Express signer.
    #[derive(Debug, Default)]
    pub(crate) struct S3ExpressSigner;

    impl Sign for S3ExpressSigner {
        fn sign_http_request(
            &self,
            request: &mut HttpRequest,
            identity: &Identity,
            auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
            runtime_components: &RuntimeComponents,
            config_bag: &ConfigBag,
        ) -> Result<(), BoxError> {
            let operation_config =
                SigV4Signer::extract_operation_config(auth_scheme_endpoint_config, config_bag)?;
            let mut settings = SigV4Signer::signing_settings(&operation_config);
            override_session_token_name(&mut settings)?;

            SigV4Signer.sign_http_request(
                request,
                identity,
                settings,
                &operation_config,
                runtime_components,
                config_bag,
            )
        }
    }

    fn override_session_token_name(settings: &mut SigningSettings) -> Result<(), BoxError> {
        let session_token_name_override = match settings.signature_location {
            SignatureLocation::Headers => Some("x-amz-s3session-token"),
            SignatureLocation::QueryParams => Some("X-Amz-S3session-Token"),
            _ => { return Err(BoxError::from("`SignatureLocation` adds a new variant, which needs to be handled in a separate match arm")) },
        };
        settings.session_token_name_override = session_token_name_override;
        Ok(())
    }
}

/// Supporting code for S3 Express identity cache
pub(crate) mod identity_cache {
    /// The caching implementation for S3 Express identity.
    ///
    /// While customers can either disable S3 Express itself or provide a custom S3 Express identity
    /// provider, configuring S3 Express identity cache is not supported. Thus, this is _the_
    /// implementation of S3 Express identity cache.
    #[derive(Debug)]
    pub(crate) struct S3ExpressIdentityCache;
}

/// Supporting code for S3 Express identity provider
pub(crate) mod identity_provider {
    use std::time::SystemTime;

    use crate::s3_express::identity_cache::S3ExpressIdentityCache;
    use crate::types::SessionCredentials;
    use aws_credential_types::provider::error::CredentialsError;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
    use aws_smithy_runtime_api::client::identity::{
        Identity, IdentityCacheLocation, IdentityFuture, ResolveCachedIdentity, ResolveIdentity,
    };
    use aws_smithy_runtime_api::client::interceptors::SharedInterceptor;
    use aws_smithy_runtime_api::client::runtime_components::{
        GetIdentityResolver, RuntimeComponents,
    };
    use aws_smithy_types::config_bag::ConfigBag;

    #[derive(Debug)]
    pub(crate) struct DefaultS3ExpressIdentityProvider {
        _cache: S3ExpressIdentityCache,
    }

    #[derive(Default)]
    pub(crate) struct Builder;

    impl TryFrom<SessionCredentials> for Credentials {
        type Error = BoxError;

        fn try_from(session_creds: SessionCredentials) -> Result<Self, Self::Error> {
            Ok(Credentials::new(
                session_creds.access_key_id,
                session_creds.secret_access_key,
                Some(session_creds.session_token),
                Some(SystemTime::try_from(session_creds.expiration).map_err(|_| {
                    CredentialsError::unhandled(
                        "credential expiration time cannot be represented by a SystemTime",
                    )
                })?),
                "s3express",
            ))
        }
    }

    impl DefaultS3ExpressIdentityProvider {
        pub(crate) fn builder() -> Builder {
            Builder
        }

        async fn identity<'a>(
            &'a self,
            runtime_components: &'a RuntimeComponents,
            config_bag: &'a ConfigBag,
        ) -> Result<Identity, BoxError> {
            let bucket_name = self.bucket_name(config_bag)?;

            let sigv4_identity_resolver = runtime_components
                .identity_resolver(aws_runtime::auth::sigv4::SCHEME_ID)
                .ok_or("identity resolver for sigv4 should be set for S3")?;
            let _aws_identity = runtime_components
                .identity_cache()
                .resolve_cached_identity(sigv4_identity_resolver, runtime_components, config_bag)
                .await?;

            // TODO(S3Express): use both `bucket_name` and `aws_identity` as part of `S3ExpressIdentityCache` implementation

            let express_session_credentials = self
                .express_session_credentials(bucket_name, runtime_components, config_bag)
                .await?;

            let data = Credentials::try_from(express_session_credentials)?;

            Ok(Identity::new(data.clone(), data.expiry()))
        }

        fn bucket_name<'a>(&'a self, config_bag: &'a ConfigBag) -> Result<&'a str, BoxError> {
            let params = config_bag
                .load::<EndpointResolverParams>()
                .expect("endpoint resolver params must be set");
            let params = params
                .get::<crate::config::endpoint::Params>()
                .expect("`Params` should be wrapped in `EndpointResolverParams`");
            params
                .bucket()
                .ok_or("A bucket was not set in endpoint params".into())
        }

        async fn express_session_credentials<'a>(
            &'a self,
            bucket_name: &'a str,
            runtime_components: &'a RuntimeComponents,
            config_bag: &'a ConfigBag,
        ) -> Result<SessionCredentials, BoxError> {
            let mut config_builder = crate::config::Builder::from_config_bag(config_bag);

            // inherits all runtime components from a current S3 operation but clears out
            // out interceptors configured for that operation
            let mut rc_builder = runtime_components.to_builder();
            rc_builder.set_interceptors(std::iter::empty::<SharedInterceptor>());
            config_builder.runtime_components = rc_builder;

            let client = crate::Client::from_conf(config_builder.build());
            let response = client
                .create_session()
                .bucket(bucket_name)
                .session_mode(crate::types::SessionMode::ReadWrite)
                .send()
                .await?;

            response
                .credentials
                .ok_or("no session credentials in response".into())
        }
    }

    impl Builder {
        pub(crate) fn build(self) -> DefaultS3ExpressIdentityProvider {
            DefaultS3ExpressIdentityProvider {
                _cache: S3ExpressIdentityCache,
            }
        }
    }

    impl ResolveIdentity for DefaultS3ExpressIdentityProvider {
        fn resolve_identity<'a>(
            &'a self,
            runtime_components: &'a RuntimeComponents,
            config_bag: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            IdentityFuture::new(async move { self.identity(runtime_components, config_bag).await })
        }

        fn cache_location(&self) -> IdentityCacheLocation {
            IdentityCacheLocation::IdentityResolver
        }
    }
}
