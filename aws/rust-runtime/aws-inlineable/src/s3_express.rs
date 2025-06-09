/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Supporting code for S3 Express auth
pub(crate) mod auth {
    use aws_runtime::auth::sigv4::SigV4Signer;
    use aws_smithy_runtime_api::client::auth::{AuthScheme, AuthSchemeId, Sign};
    use aws_smithy_runtime_api::client::identity::SharedIdentityResolver;
    use aws_smithy_runtime_api::client::runtime_components::GetIdentityResolver;

    /// Auth scheme ID for S3 Express.
    pub(crate) const SCHEME_ID: AuthSchemeId = AuthSchemeId::new("sigv4-s3express");

    /// S3 Express auth scheme.
    #[derive(Debug, Default)]
    pub(crate) struct S3ExpressAuthScheme {
        signer: SigV4Signer,
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
}

/// Supporting code for S3 Express identity cache
pub(crate) mod identity_cache {
    use aws_credential_types::Credentials;
    use aws_smithy_async::time::SharedTimeSource;
    use aws_smithy_runtime::expiring_cache::ExpiringCache;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::identity::Identity;
    use aws_smithy_types::DateTime;
    use fastrand::Rng;
    use hmac::{digest::FixedOutput, Hmac, Mac};
    use lru::LruCache;
    use sha2::Sha256;
    use std::fmt;
    use std::future::Future;
    use std::hash::Hash;
    use std::num::NonZeroUsize;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    pub(crate) const DEFAULT_MAX_CACHE_CAPACITY: usize = 100;
    pub(crate) const DEFAULT_BUFFER_TIME: Duration = Duration::from_secs(10);

    #[derive(Clone, Eq, PartialEq, Hash)]
    pub(crate) struct CacheKey(String);

    /// The caching implementation for S3 Express identity.
    ///
    /// While customers can either disable S3 Express itself or provide a custom S3 Express identity
    /// provider, configuring S3 Express identity cache is not supported. Thus, this is _the_
    /// implementation of S3 Express identity cache.
    pub(crate) struct S3ExpressIdentityCache {
        inner: Mutex<LruCache<CacheKey, ExpiringCache<Identity, BoxError>>>,
        time_source: SharedTimeSource,
        buffer_time: Duration,
        random_bytes: [u8; 64],
    }

    impl fmt::Debug for S3ExpressIdentityCache {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let (size, capacity) = {
                let cache = self.inner.lock().unwrap();
                (cache.len(), cache.cap())
            };
            write!(
                f,
                "S3ExpressIdentityCache {{ time_source: {:?}, buffer_time: {:?} }}, with size/capacity: {}/{}",
                self.time_source, &self.buffer_time, size, capacity,
            )
        }
    }

    impl S3ExpressIdentityCache {
        pub(crate) fn new(
            capacity: usize,
            time_source: SharedTimeSource,
            buffer_time: Duration,
        ) -> Self {
            // It'd be nice to use a cryptographically secure random generator but not necessary.
            // The cache is memory only and randomization here is mostly to obfuscate the key and
            // make it reasonable length.
            let mut rng = Rng::default();
            let mut random_bytes = [0u8; 64];
            rng.fill(&mut random_bytes);
            Self {
                inner: Mutex::new(LruCache::new(NonZeroUsize::new(capacity).unwrap())),
                time_source,
                buffer_time,
                random_bytes,
            }
        }

        pub(crate) fn key(&self, bucket_name: &str, creds: &Credentials) -> CacheKey {
            CacheKey({
                let mut mac = Hmac::<Sha256>::new_from_slice(self.random_bytes.as_slice())
                    .expect("should be created from random 64 bytes");
                let input = format!("{}{}", creds.access_key_id(), creds.secret_access_key());
                mac.update(input.as_ref());
                let mut inner = hex::encode(mac.finalize_fixed());
                inner.push_str(bucket_name);
                inner
            })
        }

        pub(crate) async fn get_or_load<F, Fut>(
            &self,
            key: CacheKey,
            loader: F,
        ) -> Result<Identity, BoxError>
        where
            F: FnOnce() -> Fut,
            Fut: Future<Output = Result<(Identity, SystemTime), BoxError>>,
        {
            let expiring_cache = {
                let mut inner = self.inner.lock().unwrap();
                inner
                    .get_or_insert_mut(key, || ExpiringCache::new(self.buffer_time))
                    .clone()
            };

            let now = self.time_source.now();

            match expiring_cache.yield_or_clear_if_expired(now).await {
                Some(identity) => {
                    tracing::debug!(
                        buffer_time=?self.buffer_time,
                        cached_expiration=?identity.expiration(),
                        now=?now,
                        "loaded identity from cache"
                    );
                    Ok(identity)
                }
                None => {
                    let start_time = self.time_source.now();
                    let identity = expiring_cache.get_or_load(loader).await?;
                    let expiration = identity
                        .expiration()
                        .ok_or("SessionCredentials` always has expiration")?;
                    let printable = DateTime::from(expiration);
                    tracing::info!(
                        new_expiration=%printable,
                        valid_for=?expiration.duration_since(self.time_source.now()).unwrap_or_default(),
                        "identity cache miss occurred; added new identity (took {:?})",
                        self.time_source.now().duration_since(start_time).unwrap_or_default()
                    );
                    Ok(identity)
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use aws_smithy_async::rt::sleep::TokioSleep;
        use aws_smithy_async::test_util::ManualTimeSource;
        use aws_smithy_runtime_api::client::identity::http::Token;
        use aws_smithy_runtime_api::client::identity::{
            IdentityFuture, ResolveIdentity, SharedIdentityResolver,
        };
        use aws_smithy_runtime_api::client::runtime_components::{
            RuntimeComponents, RuntimeComponentsBuilder,
        };
        use aws_smithy_runtime_api::shared::IntoShared;
        use aws_smithy_types::config_bag::ConfigBag;
        use futures_util::stream::FuturesUnordered;
        use std::sync::Arc;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        use tracing::info;

        fn epoch_secs(secs: u64) -> SystemTime {
            SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
        }

        fn identity_expiring_in(expired_secs: u64) -> Identity {
            let expiration = Some(epoch_secs(expired_secs));
            Identity::new(Token::new("test", expiration), expiration)
        }

        fn test_identity_resolver(
            load_list: Vec<Result<Identity, BoxError>>,
        ) -> SharedIdentityResolver {
            #[derive(Debug)]
            struct Resolver(Mutex<Vec<Result<Identity, BoxError>>>);
            impl ResolveIdentity for Resolver {
                fn resolve_identity<'a>(
                    &'a self,
                    _: &'a RuntimeComponents,
                    _config_bag: &'a ConfigBag,
                ) -> IdentityFuture<'a> {
                    let mut list = self.0.lock().unwrap();
                    if list.len() > 0 {
                        let next = list.remove(0);
                        info!("refreshing the identity to {:?}", next);
                        IdentityFuture::ready(next)
                    } else {
                        drop(list);
                        panic!("no more identities")
                    }
                }
            }

            SharedIdentityResolver::new(Resolver(Mutex::new(load_list)))
        }

        async fn load(
            identity_resolver: SharedIdentityResolver,
            runtime_components: &RuntimeComponents,
        ) -> Result<(Identity, SystemTime), BoxError> {
            let identity = identity_resolver
                .resolve_identity(&runtime_components, &ConfigBag::base())
                .await
                .unwrap();
            Ok((identity.clone(), identity.expiration().unwrap()))
        }

        async fn expect_identity<F, Fut>(
            expired_secs: u64,
            sut: &S3ExpressIdentityCache,
            key: CacheKey,
            loader: F,
        ) where
            F: FnOnce() -> Fut,
            Fut: Future<Output = Result<(Identity, SystemTime), BoxError>>,
        {
            let identity = sut.get_or_load(key, loader).await.unwrap();
            assert_eq!(Some(epoch_secs(expired_secs)), identity.expiration());
        }

        #[tokio::test]
        async fn reload_expired_test_identity() {
            let time = ManualTimeSource::new(UNIX_EPOCH);
            let runtime_components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time.clone()))
                .with_sleep_impl(Some(TokioSleep::new()))
                .build()
                .unwrap();

            let sut =
                S3ExpressIdentityCache::new(1, time.clone().into_shared(), DEFAULT_BUFFER_TIME);

            let identity_resolver = test_identity_resolver(vec![
                Ok(identity_expiring_in(1000)),
                Ok(identity_expiring_in(2000)),
            ]);

            let key = sut.key(
                "test-bucket--usw2-az1--x-s3",
                &Credentials::for_tests_with_session_token(),
            );

            // First call to the cache, populating a cache entry.
            expect_identity(1000, &sut, key.clone(), || {
                let identity_resolver = identity_resolver.clone();
                let runtime_components = runtime_components.clone();
                async move { load(identity_resolver, &runtime_components).await }
            })
            .await;

            // Testing for a cache hit by advancing time such that the updated time is before the expiration of the first identity
            // i.e. 500 < 1000.
            time.set_time(epoch_secs(500));

            expect_identity(1000, &sut, key.clone(), || async move {
                panic!("new identity should not be loaded")
            })
            .await;

            // Testing for a cache miss by advancing time such that the updated time is now after the expiration of the first identity
            // and before the expiration of the second identity i.e. 1000 < 1500 && 1500 < 2000.
            time.set_time(epoch_secs(1500));

            expect_identity(2000, &sut, key, || async move {
                load(identity_resolver, &runtime_components).await
            })
            .await;
        }

        #[test]
        fn load_contention() {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_time()
                .worker_threads(16)
                .build()
                .unwrap();

            let time = ManualTimeSource::new(epoch_secs(0));
            let runtime_components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time.clone()))
                .with_sleep_impl(Some(TokioSleep::new()))
                .build()
                .unwrap();

            let number_of_buckets = 4;
            let sut = Arc::new(S3ExpressIdentityCache::new(
                number_of_buckets,
                time.clone().into_shared(),
                DEFAULT_BUFFER_TIME,
            ));

            // Nested for loops below advance time by 200 in total, and each identity has the expiration
            // such that no matter what order async tasks are executed, it never expires.
            let safe_expiration = number_of_buckets as u64 * 50 + DEFAULT_BUFFER_TIME.as_secs() + 1;
            let identity_resolver = test_identity_resolver(vec![
                Ok(identity_expiring_in(safe_expiration)),
                Ok(identity_expiring_in(safe_expiration)),
                Ok(identity_expiring_in(safe_expiration)),
                Ok(identity_expiring_in(safe_expiration)),
            ]);

            let mut tasks = Vec::new();
            for i in 0..number_of_buckets {
                let key = sut.key(
                    &format!("test-bucket-{i}-usw2-az1--x-s3"),
                    &Credentials::for_tests_with_session_token(),
                );
                for _ in 0..50 {
                    let sut = sut.clone();
                    let key = key.clone();
                    let identity_resolver = identity_resolver.clone();
                    let time = time.clone();
                    let runtime_components = runtime_components.clone();
                    tasks.push(rt.spawn(async move {
                        let now = time.advance(Duration::from_secs(1));
                        let identity: Identity = sut
                            .get_or_load(key, || async move {
                                load(identity_resolver, &runtime_components).await
                            })
                            .await
                            .unwrap();

                        assert!(
                            identity.expiration().unwrap() >= now,
                            "{:?} >= {:?}",
                            identity.expiration(),
                            now
                        );
                    }));
                }
            }
            let tasks = tasks.into_iter().collect::<FuturesUnordered<_>>();
            for task in tasks {
                rt.block_on(task).unwrap();
            }
        }

        #[tokio::test]
        async fn identity_fetch_triggered_by_lru_eviction() {
            let time = ManualTimeSource::new(UNIX_EPOCH);
            let runtime_components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time.clone()))
                .with_sleep_impl(Some(TokioSleep::new()))
                .build()
                .unwrap();

            // Create a cache of size 2.
            let sut = S3ExpressIdentityCache::new(2, time.into_shared(), DEFAULT_BUFFER_TIME);

            let identity_resolver = test_identity_resolver(vec![
                Ok(identity_expiring_in(1000)),
                Ok(identity_expiring_in(2000)),
                Ok(identity_expiring_in(3000)),
                Ok(identity_expiring_in(4000)),
            ]);

            let [key1, key2, key3] = [1, 2, 3].map(|i| {
                sut.key(
                    &format!("test-bucket-{i}--usw2-az1--x-s3"),
                    &Credentials::for_tests_with_session_token(),
                )
            });

            // This should pupulate a cache entry for `key1`.
            expect_identity(1000, &sut, key1.clone(), || {
                let identity_resolver = identity_resolver.clone();
                let runtime_components = runtime_components.clone();
                async move { load(identity_resolver, &runtime_components).await }
            })
            .await;
            // This immediate next call for `key1` should be a cache hit.
            expect_identity(1000, &sut, key1.clone(), || async move {
                panic!("new identity should not be loaded")
            })
            .await;

            // This should pupulate a cache entry for `key2`.
            expect_identity(2000, &sut, key2, || {
                let identity_resolver = identity_resolver.clone();
                let runtime_components = runtime_components.clone();
                async move { load(identity_resolver, &runtime_components).await }
            })
            .await;

            // This should pupulate a cache entry for `key3`, but evicting a cache entry for `key1` because the cache is full.
            expect_identity(3000, &sut, key3.clone(), || {
                let identity_resolver = identity_resolver.clone();
                let runtime_components = runtime_components.clone();
                async move { load(identity_resolver, &runtime_components).await }
            })
            .await;

            // Attempt to get an identity for `key1` should end up fetching a new one since its cache entry has been evicted.
            // This fetch should now evict a cache entry for `key2`.
            expect_identity(4000, &sut, key1, || async move {
                load(identity_resolver, &runtime_components).await
            })
            .await;

            // A cache entry for `key3` should still exist in the cache.
            expect_identity(3000, &sut, key3, || async move {
                panic!("new identity should not be loaded")
            })
            .await;
        }
    }
}
/// Supporting code for S3 Express identity provider
pub(crate) mod identity_provider {
    use std::time::{Duration, SystemTime};

    use crate::s3_express::identity_cache::S3ExpressIdentityCache;
    use crate::types::SessionCredentials;
    use aws_credential_types::provider::error::CredentialsError;
    use aws_credential_types::Credentials;
    use aws_smithy_async::time::{SharedTimeSource, TimeSource};
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
    use aws_smithy_runtime_api::client::identity::{
        Identity, IdentityCacheLocation, IdentityFuture, ResolveCachedIdentity, ResolveIdentity,
    };
    use aws_smithy_runtime_api::client::interceptors::SharedInterceptor;
    use aws_smithy_runtime_api::client::runtime_components::{
        GetIdentityResolver, RuntimeComponents,
    };
    use aws_smithy_runtime_api::shared::IntoShared;
    use aws_smithy_types::config_bag::ConfigBag;

    use super::identity_cache::{DEFAULT_BUFFER_TIME, DEFAULT_MAX_CACHE_CAPACITY};

    #[derive(Debug)]
    pub(crate) struct DefaultS3ExpressIdentityProvider {
        behavior_version: crate::config::BehaviorVersion,
        cache: S3ExpressIdentityCache,
    }

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
            Builder::default()
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
            let aws_identity = runtime_components
                .identity_cache()
                .resolve_cached_identity(sigv4_identity_resolver, runtime_components, config_bag)
                .await?;

            let credentials = aws_identity.data::<Credentials>().ok_or(
                "wrong identity type for SigV4. Expected AWS credentials but got `{identity:?}",
            )?;

            let key = self.cache.key(bucket_name, credentials);
            self.cache
                .get_or_load(key, || async move {
                    let creds = self
                        .express_session_credentials(bucket_name, runtime_components, config_bag)
                        .await?;
                    let data = Credentials::try_from(creds)?;
                    Ok((
                        Identity::new(data.clone(), data.expiry()),
                        data.expiry().unwrap(),
                    ))
                })
                .await
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
            let mut config_builder = crate::config::Builder::from_config_bag(config_bag)
                .behavior_version(self.behavior_version);

            // inherits all runtime components from a current S3 operation but clears out
            // out interceptors configured for that operation
            let mut rc_builder = runtime_components.to_builder();
            rc_builder.set_interceptors(std::iter::empty::<SharedInterceptor>());
            config_builder.runtime_components = rc_builder;

            let client = crate::Client::from_conf(config_builder.build());
            let response = client.create_session().bucket(bucket_name).send().await?;

            response
                .credentials
                .ok_or("no session credentials in response".into())
        }
    }

    #[derive(Default)]
    pub(crate) struct Builder {
        behavior_version: Option<crate::config::BehaviorVersion>,
        time_source: Option<SharedTimeSource>,
        buffer_time: Option<Duration>,
    }

    impl Builder {
        pub(crate) fn behavior_version(
            mut self,
            behavior_version: crate::config::BehaviorVersion,
        ) -> Self {
            self.set_behavior_version(Some(behavior_version));
            self
        }
        pub(crate) fn set_behavior_version(
            &mut self,
            behavior_version: Option<crate::config::BehaviorVersion>,
        ) -> &mut Self {
            self.behavior_version = behavior_version;
            self
        }
        pub(crate) fn time_source(mut self, time_source: impl TimeSource + 'static) -> Self {
            self.set_time_source(time_source.into_shared());
            self
        }
        pub(crate) fn set_time_source(&mut self, time_source: SharedTimeSource) -> &mut Self {
            self.time_source = Some(time_source.into_shared());
            self
        }
        #[allow(dead_code)]
        pub(crate) fn buffer_time(mut self, buffer_time: Duration) -> Self {
            self.set_buffer_time(Some(buffer_time));
            self
        }
        #[allow(dead_code)]
        pub(crate) fn set_buffer_time(&mut self, buffer_time: Option<Duration>) -> &mut Self {
            self.buffer_time = buffer_time;
            self
        }
        pub(crate) fn build(self) -> DefaultS3ExpressIdentityProvider {
            DefaultS3ExpressIdentityProvider {
                behavior_version: self
                    .behavior_version
                    .expect("required field `behavior_version` should be set"),
                cache: S3ExpressIdentityCache::new(
                    DEFAULT_MAX_CACHE_CAPACITY,
                    self.time_source.unwrap_or_default(),
                    self.buffer_time.unwrap_or(DEFAULT_BUFFER_TIME),
                ),
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

/// Supporting code for S3 Express runtime plugin
pub(crate) mod runtime_plugin {
    use std::borrow::Cow;

    use aws_runtime::auth::SigV4SessionTokenNameOverride;
    use aws_sigv4::http_request::{SignatureLocation, SigningSettings};
    use aws_smithy_runtime_api::{
        box_error::BoxError,
        client::{runtime_components::RuntimeComponentsBuilder, runtime_plugin::RuntimePlugin},
    };
    use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Layer};
    use aws_types::os_shim_internal::Env;

    mod env {
        pub(super) const S3_DISABLE_EXPRESS_SESSION_AUTH: &str =
            "AWS_S3_DISABLE_EXPRESS_SESSION_AUTH";
    }

    #[derive(Debug)]
    pub(crate) struct S3ExpressRuntimePlugin {
        config: FrozenLayer,
        runtime_components_builder: RuntimeComponentsBuilder,
    }

    impl S3ExpressRuntimePlugin {
        // `new` will be called as `additional_client_plugins` within `base_client_runtime_plugins`.
        // This guarantees that `new` receives a fully constructed service config, with required
        // runtime components registered with `RuntimeComponents`.
        pub(crate) fn new(service_config: crate::config::Config) -> Self {
            Self::new_with(service_config, Env::real())
        }

        fn new_with(service_config: crate::config::Config, env: Env) -> Self {
            Self {
                config: config(
                    service_config
                        .config
                        .load::<crate::config::DisableS3ExpressSessionAuth>()
                        .cloned(),
                    env,
                ),
                runtime_components_builder: runtime_components_builder(service_config),
            }
        }
    }

    fn config(
        disable_s3_express_session_token: Option<crate::config::DisableS3ExpressSessionAuth>,
        env: Env,
    ) -> FrozenLayer {
        let mut layer = Layer::new("S3ExpressRuntimePlugin");
        if disable_s3_express_session_token.is_none() {
            match env.get(env::S3_DISABLE_EXPRESS_SESSION_AUTH) {
                Ok(value)
                    if value.eq_ignore_ascii_case("true")
                        || value.eq_ignore_ascii_case("false") =>
                {
                    let value = value
                        .to_lowercase()
                        .parse::<bool>()
                        .expect("just checked to be a bool-valued string");
                    layer.store_or_unset(Some(crate::config::DisableS3ExpressSessionAuth(value)));
                }
                Ok(value) => {
                    tracing::warn!(
                        "environment variable `{}` ignored since it only accepts either `true` or `false` (case-insensitive), but got `{}`.",
                        env::S3_DISABLE_EXPRESS_SESSION_AUTH,
                        value
                    )
                }
                _ => {
                    // TODO(aws-sdk-rust#1073): Transfer a value of
                    //  `s3_disable_express_session_auth` from a profile file to `layer`
                }
            }
        }

        let session_token_name_override = SigV4SessionTokenNameOverride::new(
            |settings: &SigningSettings, cfg: &ConfigBag| {
                // Not configured for S3 express, use the original session token name override
                if !crate::s3_express::utils::for_s3_express(cfg) {
                    return Ok(settings.session_token_name_override);
                }

                let session_token_name_override = Some(match settings.signature_location {
                SignatureLocation::Headers => "x-amz-s3session-token",
                SignatureLocation::QueryParams => "X-Amz-S3session-Token",
                _ => {
                    return Err(BoxError::from(
                        "`SignatureLocation` adds a new variant, which needs to be handled in a separate match arm",
                    ))
                }
            });
                Ok(session_token_name_override)
            },
        );
        layer.store_or_unset(Some(session_token_name_override));

        layer.freeze()
    }

    fn runtime_components_builder(
        service_config: crate::config::Config,
    ) -> RuntimeComponentsBuilder {
        match (
            service_config
                .runtime_components
                .identity_resolver(&super::auth::SCHEME_ID),
            service_config
                .runtime_components
                .identity_resolver(&aws_runtime::auth::sigv4::SCHEME_ID),
        ) {
            (None, Some(_)) => RuntimeComponentsBuilder::new("S3ExpressRuntimePlugin")
                .with_identity_resolver(
                    super::auth::SCHEME_ID,
                    super::identity_provider::DefaultS3ExpressIdentityProvider::builder()
                        .time_source(
                            service_config
                                .runtime_components
                                .time_source()
                                .expect("should be set in `service_config`"),
                        )
                        .behavior_version(
                            service_config
                                .behavior_version
                                .expect("should be set in `service_config`"),
                        )
                        .build(),
                ),
            _ => RuntimeComponentsBuilder::new("S3ExpressRuntimePlugin"),
        }
    }

    impl RuntimePlugin for S3ExpressRuntimePlugin {
        fn config(&self) -> Option<FrozenLayer> {
            Some(self.config.clone())
        }

        fn runtime_components(
            &self,
            _: &RuntimeComponentsBuilder,
        ) -> Cow<'_, RuntimeComponentsBuilder> {
            Cow::Borrowed(&self.runtime_components_builder)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use aws_credential_types::Credentials;
        use aws_smithy_runtime_api::client::identity::ResolveIdentity;

        #[test]
        fn disable_option_set_from_service_client_should_take_the_highest_precedence() {
            // Disable option is set from service client.
            let disable_s3_express_session_token = crate::config::DisableS3ExpressSessionAuth(true);

            // An environment variable says the session auth is _not_ disabled, but it will be
            // overruled by what is in `layer`.
            let actual = config(
                Some(disable_s3_express_session_token),
                Env::from_slice(&[(super::env::S3_DISABLE_EXPRESS_SESSION_AUTH, "false")]),
            );

            // A config layer from this runtime plugin should not provide a new `DisableS3ExpressSessionAuth`
            // if the disable option is set from service client.
            assert!(actual
                .load::<crate::config::DisableS3ExpressSessionAuth>()
                .is_none());
        }

        #[test]
        fn disable_option_set_from_env_should_take_the_second_highest_precedence() {
            // An environment variable says session auth is disabled
            let actual = config(
                None,
                Env::from_slice(&[(super::env::S3_DISABLE_EXPRESS_SESSION_AUTH, "true")]),
            );

            assert!(
                actual
                    .load::<crate::config::DisableS3ExpressSessionAuth>()
                    .unwrap()
                    .0
            );
        }

        #[should_panic]
        #[test]
        fn disable_option_set_from_profile_file_should_take_the_lowest_precedence() {
            // TODO(aws-sdk-rust#1073): Implement a test that mimics only setting
            //  `s3_disable_express_session_auth` in a profile file
            todo!()
        }

        #[test]
        fn disable_option_should_be_unspecified_if_unset() {
            let actual = config(None, Env::from_slice(&[]));

            assert!(actual
                .load::<crate::config::DisableS3ExpressSessionAuth>()
                .is_none());
        }

        #[test]
        fn s3_express_runtime_plugin_should_set_default_identity_resolver() {
            let config = crate::Config::builder()
                .behavior_version_latest()
                .time_source(aws_smithy_async::time::SystemTimeSource::new())
                .credentials_provider(Credentials::for_tests())
                .build();

            let actual = runtime_components_builder(config);
            assert!(actual
                .identity_resolver(&crate::s3_express::auth::SCHEME_ID)
                .is_some());
        }

        #[test]
        fn s3_express_plugin_should_not_set_default_identity_resolver_without_sigv4_counterpart() {
            let config = crate::Config::builder()
                .behavior_version_latest()
                .time_source(aws_smithy_async::time::SystemTimeSource::new())
                .build();

            let actual = runtime_components_builder(config);
            assert!(actual
                .identity_resolver(&crate::s3_express::auth::SCHEME_ID)
                .is_none());
        }

        #[tokio::test]
        async fn s3_express_plugin_should_not_set_default_identity_resolver_if_user_provided() {
            let expected_access_key_id = "expected acccess key ID";
            let config = crate::Config::builder()
                .behavior_version_latest()
                .credentials_provider(Credentials::for_tests())
                .express_credentials_provider(Credentials::new(
                    expected_access_key_id,
                    "secret",
                    None,
                    None,
                    "test express credentials provider",
                ))
                .time_source(aws_smithy_async::time::SystemTimeSource::new())
                .build();

            // `RuntimeComponentsBuilder` from `S3ExpressRuntimePlugin` should not provide an S3Express identity resolver.
            let runtime_components_builder = runtime_components_builder(config.clone());
            assert!(runtime_components_builder
                .identity_resolver(&crate::s3_express::auth::SCHEME_ID)
                .is_none());

            // Get the S3Express identity resolver from the service config.
            let express_identity_resolver = config
                .runtime_components
                .identity_resolver(&crate::s3_express::auth::SCHEME_ID)
                .unwrap();
            let creds = express_identity_resolver
                .resolve_identity(
                    &RuntimeComponentsBuilder::for_tests().build().unwrap(),
                    &ConfigBag::base(),
                )
                .await
                .unwrap();

            // Verify credentials are the one generated by the S3Express identity resolver user provided.
            assert_eq!(
                expected_access_key_id,
                creds.data::<Credentials>().unwrap().access_key_id()
            );
        }
    }
}

pub(crate) mod checksum {
    use crate::http_request_checksum::DefaultRequestChecksumOverride;
    use aws_smithy_checksums::ChecksumAlgorithm;
    use aws_smithy_types::config_bag::ConfigBag;

    pub(crate) fn provide_default_checksum_algorithm(
    ) -> crate::http_request_checksum::DefaultRequestChecksumOverride {
        fn _provide_default_checksum_algorithm(
            original_checksum: Option<ChecksumAlgorithm>,
            cfg: &ConfigBag,
        ) -> Option<ChecksumAlgorithm> {
            // S3 does not have the `ChecksumAlgorithm::Md5`, therefore customers cannot set it
            // from outside.
            if original_checksum != Some(ChecksumAlgorithm::Md5) {
                return original_checksum;
            }

            if crate::s3_express::utils::for_s3_express(cfg) {
                // S3 Express requires setting the default checksum algorithm to CRC-32
                Some(ChecksumAlgorithm::Crc32)
            } else {
                original_checksum
            }
        }
        DefaultRequestChecksumOverride::new(_provide_default_checksum_algorithm)
    }
}

pub(crate) mod utils {
    use aws_smithy_types::{config_bag::ConfigBag, Document};

    pub(crate) fn for_s3_express(cfg: &ConfigBag) -> bool {
        // logic borrowed from aws_smithy_runtime::client::orchestrator::auth::extract_endpoint_auth_scheme_config
        let endpoint = cfg
            .load::<crate::config::endpoint::Endpoint>()
            .expect("endpoint added to config bag by endpoint orchestrator");

        let auth_schemes = match endpoint.properties().get("authSchemes") {
            Some(Document::Array(schemes)) => schemes,
            _ => return false,
        };
        auth_schemes.iter().any(|doc| {
            let config_scheme_id = doc
                .as_object()
                .and_then(|object| object.get("name"))
                .and_then(Document::as_string);
            config_scheme_id == Some(crate::s3_express::auth::SCHEME_ID.as_str())
        })
    }
}
