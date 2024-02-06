/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Supporting code for S3 Express auth
pub(crate) mod auth {
    use std::borrow::Cow;

    use aws_credential_types::Credentials;
    use aws_runtime::auth::sigv4::SigV4Signer;
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
            if let Some(excluded) = settings.excluded_headers.as_mut() {
                excluded.push(Cow::Borrowed("x-amz-security-token"));
            }

            let express_credentials = identity.data::<Credentials>().ok_or(
                "wrong identity type for SigV4. Expected AWS credentials but got `{identity:?}",
            )?;
            let mut value = http::HeaderValue::from_str(
                express_credentials
                    .session_token()
                    .expect("S3 session token should be set"),
            )
            .unwrap();
            value.set_sensitive(true);
            request.headers_mut().insert(
                http::HeaderName::from_static("x-amz-s3session-token"),
                value,
            );

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
}

/// Supporting code for S3 Express identity cache
pub(crate) mod identity_cache {
    use aws_credential_types::Credentials;
    use aws_smithy_async::time::SharedTimeSource;
    use aws_smithy_runtime::expiring_cache::ExpiringCache;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::identity::Identity;
    use hmac::{digest::FixedOutput, Hmac, Mac};
    use lru::LruCache;
    use sha2::Sha256;
    use std::fmt;
    use std::future::Future;
    use std::hash::Hash;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    pub(crate) const DEFAULT_MAX_CACHE_CAPACITY: usize = 100;
    pub(crate) const DEFAULT_BUFFER_TIME: Duration = Duration::from_secs(10);

    #[derive(Clone, Eq, PartialEq, Hash)]
    pub(crate) struct CacheKey(String);

    impl CacheKey {
        pub(crate) fn from(bucket_name: &str, creds: &Credentials) -> Self {
            Self({
                let key = format!("{}{}", creds.access_key_id(), creds.secret_access_key());
                let mac = Hmac::<Sha256>::new_from_slice(key.as_ref())
                    .expect("HMAC can take key of any size");
                let mut inner = hex::encode(mac.finalize_fixed());
                inner.push_str(bucket_name);
                inner
            })
        }
    }

    /// The caching implementation for S3 Express identity.
    ///
    /// While customers can either disable S3 Express itself or provide a custom S3 Express identity
    /// provider, configuring S3 Express identity cache is not supported. Thus, this is _the_
    /// implementation of S3 Express identity cache.
    pub(crate) struct S3ExpressIdentityCache {
        pub(crate) inner: Mutex<LruCache<CacheKey, ExpiringCache<Identity, BoxError>>>,
        pub(crate) time_source: SharedTimeSource,
        pub(crate) buffer_time: Duration,
    }

    impl fmt::Debug for S3ExpressIdentityCache {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("S3ExpressIdentityCache")
                .field("time_source", &self.time_source)
                .field("buffer_time", &self.buffer_time)
                .finish()
        }
    }

    impl S3ExpressIdentityCache {
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
                    let identity = expiring_cache.get_or_load(loader).await;
                    tracing::info!(
                        "identity cache miss occurred; added new identity (took {:?})",
                        self.time_source.now().duration_since(start_time)
                    );
                    identity
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
        use std::num::NonZeroUsize;
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

            let sut = S3ExpressIdentityCache {
                inner: Mutex::new(LruCache::new(NonZeroUsize::new(1).unwrap())),
                time_source: time.clone().into_shared(),
                buffer_time: DEFAULT_BUFFER_TIME,
            };

            let identity_resolver =
                test_identity_resolver(vec![Ok(test_identity(1000)), Ok(test_identity(2000))]);

            let key = CacheKey::from(
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
            let sut = Arc::new(S3ExpressIdentityCache {
                inner: Mutex::new(LruCache::new(NonZeroUsize::new(number_of_buckets).unwrap())),
                time_source: time.clone().into_shared(),
                buffer_time: DEFAULT_BUFFER_TIME,
            });

            // Nested for loops below advance time by 200 in total, and each identity has the expiration
            // such that no matter what order async tasks are executed, it never expires.
            let safe_expiration = number_of_buckets as u64 * 50 + DEFAULT_BUFFER_TIME.as_secs() + 1;
            let identity_resolver = test_identity_resolver(vec![
                Ok(test_identity(safe_expiration)),
                Ok(test_identity(safe_expiration)),
                Ok(test_identity(safe_expiration)),
                Ok(test_identity(safe_expiration)),
            ]);

            let mut tasks = Vec::new();
            for i in 0..number_of_buckets {
                let key = CacheKey::from(
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
            let sut = S3ExpressIdentityCache {
                inner: Mutex::new(LruCache::new(NonZeroUsize::new(2).unwrap())),
                time_source: time.into_shared(),
                buffer_time: DEFAULT_BUFFER_TIME,
            };

            let identity_resolver = test_identity_resolver(vec![
                Ok(test_identity(1000)),
                Ok(test_identity(2000)),
                Ok(test_identity(3000)),
                Ok(test_identity(4000)),
            ]);

            let [key1, key2, key3] = [1, 2, 3].map(|i| {
                CacheKey::from(
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
    use std::num::NonZeroUsize;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    use crate::s3_express::identity_cache::{CacheKey, S3ExpressIdentityCache};
    use crate::types::SessionCredentials;
    use aws_credential_types::provider::error::CredentialsError;
    use aws_credential_types::Credentials;
    use aws_smithy_async::time::{SharedTimeSource, TimeSource};
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
    use aws_smithy_runtime_api::client::identity::{
        Identity, IdentityFuture, ResolveCachedIdentity, ResolveIdentity,
    };
    use aws_smithy_runtime_api::client::interceptors::SharedInterceptor;
    use aws_smithy_runtime_api::client::runtime_components::{
        GetIdentityResolver, RuntimeComponents,
    };
    use aws_smithy_runtime_api::shared::IntoShared;
    use aws_smithy_types::config_bag::ConfigBag;
    use lru::LruCache;

    use super::identity_cache::{DEFAULT_BUFFER_TIME, DEFAULT_MAX_CACHE_CAPACITY};

    #[derive(Debug)]
    pub(crate) struct DefaultS3ExpressIdentityProvider {
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
            let bucket_name = self.bucket_name(config_bag);

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

            let key = CacheKey::from(bucket_name, credentials);
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

        fn bucket_name<'a>(&'a self, config_bag: &'a ConfigBag) -> &'a str {
            let params = config_bag
                .load::<EndpointResolverParams>()
                .expect("endpoint resolver params must be set");
            let params = params
                .get::<crate::config::endpoint::Params>()
                .expect("`Params` should be wrapped in `EndpointResolverParams`");
            params.bucket().expect("bucket name must be set")
        }

        async fn express_session_credentials<'a>(
            &'a self,
            bucket_name: &'a str,
            runtime_components: &'a RuntimeComponents,
            config_bag: &'a ConfigBag,
        ) -> Result<SessionCredentials, BoxError> {
            let mut config_builder = crate::Config::builder();
            config_builder.set_accelerate(
                config_bag
                    .load::<crate::config::Accelerate>()
                    .map(|ty| ty.0),
            );
            config_builder.set_app_name(config_bag.load::<aws_types::app_name::AppName>().cloned());
            config_builder.set_disable_multi_region_access_points(
                config_bag
                    .load::<crate::config::DisableMultiRegionAccessPoints>()
                    .map(|ty| ty.0),
            );
            config_builder.set_endpoint_url(
                config_bag
                    .load::<::aws_types::endpoint_config::EndpointUrl>()
                    .map(|ty| ty.0.clone()),
            );
            config_builder.set_force_path_style(
                config_bag
                    .load::<crate::config::ForcePathStyle>()
                    .map(|ty| ty.0),
            );
            config_builder.set_region(config_bag.load::<::aws_types::region::Region>().cloned());
            config_builder.set_retry_config(
                config_bag
                    .load::<aws_smithy_types::retry::RetryConfig>()
                    .cloned(),
            );
            config_builder.set_retry_partition(
                config_bag
                    .load::<::aws_smithy_runtime::client::retries::RetryPartition>()
                    .cloned(),
            );
            config_builder.set_timeout_config(
                config_bag
                    .load::<::aws_smithy_types::timeout::TimeoutConfig>()
                    .cloned(),
            );
            config_builder.set_use_arn_region(
                config_bag
                    .load::<crate::config::UseArnRegion>()
                    .map(|ty| ty.0),
            );
            config_builder.set_use_dual_stack(
                config_bag
                    .load::<::aws_types::endpoint_config::UseDualStack>()
                    .map(|ty| ty.0),
            );
            config_builder.set_use_fips(
                config_bag
                    .load::<::aws_types::endpoint_config::UseFips>()
                    .map(|ty| ty.0),
            );

            // inherits all runtime components from a current S3 operation but clears out
            // out interceptors configured for that operation
            let mut builder = runtime_components.to_builder();
            builder.set_interceptors(std::iter::empty::<SharedInterceptor>());
            config_builder.runtime_components = builder;

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

    #[derive(Default)]
    pub(crate) struct Builder {
        time_source: Option<SharedTimeSource>,
        buffer_time: Option<Duration>,
    }

    impl Builder {
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
                cache: S3ExpressIdentityCache {
                    inner: Mutex::new(LruCache::new(
                        NonZeroUsize::new(DEFAULT_MAX_CACHE_CAPACITY).unwrap(),
                    )),
                    time_source: self.time_source.unwrap_or_default(),
                    buffer_time: self.buffer_time.unwrap_or(DEFAULT_BUFFER_TIME),
                },
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
    }
}
