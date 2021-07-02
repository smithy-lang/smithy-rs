/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::provider::{AsyncProvideCredentials, BoxFuture, CredentialsResult};
use crate::Credentials;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{OnceCell, RwLock};
use tracing::{trace_span, warn};

const DEFAULT_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CREDENTIAL_EXPIRATION: Duration = Duration::from_secs(15 * 60);

// TODO: Implement async runtime-agnostic timeouts
// TODO: Add catch_unwind() to handle panics
// TODO: Update doc comment below once catch_unwind and timeouts are implemented
// TODO: Update warning not to use this in the STS example once it's prod ready

/// `LazyCachingCredentialsProvider` implements [`AsyncProvideCredentials`] by caching
/// credentials that it loads by calling a user-provided [`AsyncProvideCredentials`] implementation.
///
/// For example, you can provide an [`AsyncProvideCredentials`] implementation that calls
/// AWS STS's AssumeRole operation to get temporary credentials, and `LazyCachingCredentialsProvider`
/// will cache those credentials until they expire.
///
/// # Note
///
/// This is __NOT__ production ready yet. Timeouts and panic safety have not been implemented yet.
pub struct LazyCachingCredentialsProvider(Provider<SystemTimeProvider>);

impl LazyCachingCredentialsProvider {
    fn new(
        refresh: Arc<dyn AsyncProvideCredentials>,
        refresh_timeout: Duration,
        default_credential_expiration: Duration,
    ) -> Self {
        LazyCachingCredentialsProvider(Provider::new(
            SystemTimeProvider,
            refresh,
            refresh_timeout,
            default_credential_expiration,
        ))
    }

    /// Returns a new `Builder` that can be used to construct the `LazyCachingCredentialsProvider`.
    pub fn builder() -> builder::Builder {
        builder::Builder::new()
    }
}

impl AsyncProvideCredentials for LazyCachingCredentialsProvider {
    fn provide_credentials(&self) -> BoxFuture<CredentialsResult> {
        self.0.provide_credentials()
    }
}

pub mod builder {
    use crate::provider::lazy_caching::{
        LazyCachingCredentialsProvider, DEFAULT_CREDENTIAL_EXPIRATION, DEFAULT_REFRESH_TIMEOUT,
    };
    use crate::provider::AsyncProvideCredentials;
    use std::sync::Arc;
    use std::time::Duration;

    /// Builder for constructing a [`LazyCachingCredentialsProvider`].
    ///
    /// # Example
    ///
    /// ```
    /// use aws_auth::Credentials;
    /// use aws_auth::provider::async_provide_credentials_fn;
    /// use aws_auth::provider::lazy_caching::LazyCachingCredentialsProvider;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let provider = LazyCachingCredentialsProvider::builder()
    ///     .refresh(async_provide_credentials_fn(|| async {
    ///         // An async process to retrieve credentials would go here:
    ///         Ok(Credentials::from_keys("example", "example", None))
    ///     }))
    ///     .refresh_timeout(Duration::from_secs(30))
    ///     .build();
    /// ```
    #[derive(Default)]
    pub struct Builder {
        refresh: Option<Arc<dyn AsyncProvideCredentials>>,
        refresh_timeout: Option<Duration>,
        default_credential_expiration: Option<Duration>,
    }

    impl Builder {
        pub fn new() -> Self {
            Default::default()
        }

        /// An implementation of [`AsyncProvideCredentials`] that will be used to refresh
        /// the cached credentials once they're expired.
        pub fn refresh(mut self, refresh: impl AsyncProvideCredentials + 'static) -> Self {
            self.refresh = Some(Arc::new(refresh));
            self
        }

        /// (Optional) Timeout for the given [`AsyncProvideCredentials`] implementation.
        /// Defaults to 5 seconds.
        pub fn refresh_timeout(mut self, timeout: Duration) -> Self {
            self.refresh_timeout = Some(timeout);
            self
        }

        /// (Optional) Default expiration time to set on credentials if they don't
        /// have an expiration time. This is only used if the given [`AsyncProvideCredentials`]
        /// returns [`Credentials`](crate::Credentials) that don't have their `expiry` set.
        /// This must be at least 15 minutes.
        pub fn default_credential_expiration(mut self, duration: Duration) -> Self {
            self.default_credential_expiration = Some(duration);
            self
        }

        /// Creates the [`LazyCachingCredentialsProvider`].
        pub fn build(self) -> LazyCachingCredentialsProvider {
            let default_credential_expiration = self
                .default_credential_expiration
                .unwrap_or(DEFAULT_CREDENTIAL_EXPIRATION);
            assert!(
                default_credential_expiration >= DEFAULT_CREDENTIAL_EXPIRATION,
                "default_credential_expiration must be at least 15 minutes"
            );
            LazyCachingCredentialsProvider::new(
                self.refresh.expect("refresh provider is required"),
                self.refresh_timeout.unwrap_or(DEFAULT_REFRESH_TIMEOUT),
                default_credential_expiration,
            )
        }
    }
}

// Allows us to abstract time for tests.
trait TimeProvider: Clone + Send + Sync + 'static {
    fn now(&self) -> SystemTime;
}

#[derive(Copy, Clone)]
struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Returns whether or not the `credentials` are expired based on the given time.
/// This will panic if the credentials don't have an `expiry` set.
fn expired(credentials: &Credentials, now: SystemTime) -> bool {
    let expiration = credentials
        .expiry()
        .expect("refresh sets expiry if not given");
    now > expiration
}

#[derive(Clone)]
struct Inner<T: TimeProvider> {
    time: T,
    cache: Cache,
    refresh: Arc<dyn AsyncProvideCredentials>,
    refresh_timeout: Duration,
    default_credential_expiration: Duration,
}

impl<T: TimeProvider> Inner<T> {
    async fn refresh(&self) -> CredentialsResult {
        let time = self.time.clone();
        let default_credential_expiration = self.default_credential_expiration;
        let future = self.refresh.provide_credentials();
        self.cache
            .refresh(|| async move {
                let credentials = future.await?;
                // If the credentials don't have an expiration time, then create a default one
                let credentials = if credentials.expiry().is_none() {
                    Credentials::new(
                        credentials.access_key_id(),
                        credentials.secret_access_key(),
                        credentials.session_token().map(|s| s.to_string()),
                        Some(time.now() + default_credential_expiration),
                        "lazy_caching_default_credential_expiration",
                    )
                } else {
                    credentials
                };
                Ok(credentials)
            })
            .await
    }

    async fn needs_refresh(&self, now: SystemTime) -> bool {
        if let Some(credentials) = self.cache.get().await {
            if expired(&credentials, now) {
                self.cache.clear_if_expired(now).await
            } else {
                false
            }
        } else {
            true
        }
    }

    async fn cached(&self) -> Credentials {
        self.cache
            .get()
            .await
            .expect("refresh requirement checked in advance")
    }
}

struct Provider<T: TimeProvider> {
    inner: Inner<T>,
}

impl<T: TimeProvider> Provider<T> {
    fn new(
        time: T,
        refresh: Arc<dyn AsyncProvideCredentials>,
        refresh_timeout: Duration,
        default_credential_expiration: Duration,
    ) -> Self {
        Provider {
            inner: Inner {
                time,
                cache: Cache::new(),
                refresh,
                refresh_timeout,
                default_credential_expiration,
            },
        }
    }

    fn provide_credentials(&self) -> BoxFuture<CredentialsResult> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let now = inner.time.now();
            if inner.needs_refresh(now).await {
                let span = trace_span!("lazy_refresh_credentials");
                let _enter = span.enter();
                inner.refresh().await
            } else {
                Ok(inner.cached().await)
            }
        })
    }
}

#[derive(Clone)]
struct Cache {
    value: Arc<RwLock<OnceCell<Credentials>>>,
}

impl Cache {
    pub fn new() -> Cache {
        Cache {
            value: Arc::new(RwLock::new(OnceCell::new())),
        }
    }

    pub async fn get(&self) -> Option<Credentials> {
        self.value.read().await.get().cloned()
    }

    pub async fn refresh<F, Fut>(&self, f: F) -> CredentialsResult
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = CredentialsResult>,
    {
        let lock = self.value.read().await;
        let future = lock.get_or_try_init(f);
        future.await.map(|creds| creds.clone())
    }

    /// Returns true if the cache was cleared
    pub async fn clear_if_expired(&self, now: SystemTime) -> bool {
        let mut lock = self.value.write().await;

        // Only clear the cache if it hasn't been cleared by another thread. If it was already
        // cleared, then another thread is initializing the empty cell.
        if let Some(credentials) = lock.get() {
            let should_clear = credentials
                .expiry()
                .map(|expiration| now > expiration)
                .unwrap_or({
                    warn!("Cached credentials don't have an expiration time. This is a bug in aws-auth.");
                    false
                });
            if should_clear {
                *lock = OnceCell::new();
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::expired;
    use crate::provider::lazy_caching::{
        Cache, Provider, TimeProvider, DEFAULT_CREDENTIAL_EXPIRATION, DEFAULT_REFRESH_TIMEOUT,
    };
    use crate::provider::{async_provide_credentials_fn, CredentialsError, CredentialsResult};
    use crate::Credentials;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};
    use tracing::info;

    #[derive(Clone)]
    struct TestTime {
        time: Arc<Mutex<SystemTime>>,
    }

    impl TestTime {
        fn new(time: SystemTime) -> Self {
            TestTime {
                time: Arc::new(Mutex::new(time)),
            }
        }

        fn set(&self, time: SystemTime) {
            *self.time.lock().unwrap() = time;
        }
    }

    impl TimeProvider for TestTime {
        fn now(&self) -> SystemTime {
            *self.time.lock().unwrap()
        }
    }

    fn test_provider<T: TimeProvider>(
        time: T,
        refresh_list: Vec<CredentialsResult>,
    ) -> Provider<T> {
        let refresh_list = Arc::new(Mutex::new(refresh_list));
        Provider::new(
            time,
            Arc::new(async_provide_credentials_fn(move || {
                let list = refresh_list.clone();
                async move {
                    let next = list.lock().unwrap().remove(0);
                    info!("refreshing the credentials to {:?}", next);
                    next
                }
            })),
            DEFAULT_REFRESH_TIMEOUT,
            DEFAULT_CREDENTIAL_EXPIRATION,
        )
    }

    fn epoch_secs(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn credentials(expired_secs: u64) -> Credentials {
        Credentials::new("test", "test", None, Some(epoch_secs(expired_secs)), "test")
    }

    async fn expect_creds<T: TimeProvider>(expired_secs: u64, provider: &Provider<T>) {
        let creds = provider
            .provide_credentials()
            .await
            .expect("expected credentials");
        assert_eq!(Some(epoch_secs(expired_secs)), creds.expiry());
    }

    #[test]
    fn expired_check() {
        let creds = credentials(100);
        assert!(expired(&creds, epoch_secs(1000)));
        assert!(!expired(&creds, epoch_secs(10)));
    }

    #[test_env_log::test(tokio::test)]
    async fn cache_clears_if_expired_only() {
        let cache = Cache::new();
        assert!(!cache.clear_if_expired(epoch_secs(100)).await);

        cache
            .refresh(|| async { Ok(credentials(100)) })
            .await
            .unwrap();
        assert_eq!(Some(epoch_secs(100)), cache.get().await.unwrap().expiry());

        // It should not clear the credentials if they're not expired
        assert!(!cache.clear_if_expired(epoch_secs(10)).await);
        assert_eq!(Some(epoch_secs(100)), cache.get().await.unwrap().expiry());

        // It should clear the credentials if they're expired
        assert!(cache.clear_if_expired(epoch_secs(500)).await);
        assert!(cache.get().await.is_none());
    }

    #[test_env_log::test(tokio::test)]
    async fn initial_populate_credentials() {
        let time = TestTime::new(epoch_secs(100));
        let refresh = Arc::new(async_provide_credentials_fn(|| async {
            info!("refreshing the credentials");
            Ok(credentials(1000))
        }));
        let provider = Provider::new(
            time,
            refresh,
            DEFAULT_REFRESH_TIMEOUT,
            DEFAULT_CREDENTIAL_EXPIRATION,
        );
        assert_eq!(
            epoch_secs(1000),
            provider
                .provide_credentials()
                .await
                .unwrap()
                .expiry()
                .unwrap()
        );
    }

    #[test_env_log::test(tokio::test)]
    async fn refresh_expired_credentials() {
        let provider = test_provider(
            TestTime::new(epoch_secs(100)),
            vec![
                Ok(credentials(1000)),
                Ok(credentials(2000)),
                Ok(credentials(3000)),
            ],
        );

        expect_creds(1000, &provider).await;
        expect_creds(1000, &provider).await;
        provider.inner.time.set(epoch_secs(1500));
        expect_creds(2000, &provider).await;
        expect_creds(2000, &provider).await;
        provider.inner.time.set(epoch_secs(2500));
        expect_creds(3000, &provider).await;
        expect_creds(3000, &provider).await;
    }

    #[test_env_log::test(tokio::test)]
    async fn refresh_failed_error() {
        let provider = test_provider(
            TestTime::new(epoch_secs(100)),
            vec![
                Ok(credentials(1000)),
                Err(CredentialsError::CredentialsNotLoaded),
            ],
        );

        expect_creds(1000, &provider).await;
        provider.inner.time.set(epoch_secs(1500));
        assert!(provider.provide_credentials().await.is_err());
    }
}
