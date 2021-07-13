/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::provider::cache::Cache;
use crate::provider::time::{SystemTimeSource, TimeSource};
use crate::provider::{AsyncProvideCredentials, BoxFuture, CredentialsResult};
use std::sync::Arc;
use std::time::Duration;
use tracing::{trace_span, Instrument};

const DEFAULT_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CREDENTIAL_EXPIRATION: Duration = Duration::from_secs(15 * 60);
const DEFAULT_EXPIRATION_MARGIN: Duration = Duration::from_secs(10);

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
pub struct LazyCachingCredentialsProvider(Provider<SystemTimeSource>);

impl LazyCachingCredentialsProvider {
    fn new(
        refresh: Arc<dyn AsyncProvideCredentials>,
        refresh_timeout: Duration,
        default_credential_expiration: Duration,
        expiration_margin: Duration,
    ) -> Self {
        LazyCachingCredentialsProvider(Provider::new(
            SystemTimeSource,
            refresh,
            refresh_timeout,
            default_credential_expiration,
            expiration_margin,
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
        LazyCachingCredentialsProvider, DEFAULT_CREDENTIAL_EXPIRATION, DEFAULT_EXPIRATION_MARGIN,
        DEFAULT_REFRESH_TIMEOUT,
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
        expiration_margin: Option<Duration>,
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

        /// (Optional) Amount of time before the actual credential expiration time
        /// where credentials are considered expired. Defaults to 10 seconds.
        pub fn expiration_margin(mut self, margin: Duration) -> Self {
            self.expiration_margin = Some(margin);
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
                self.expiration_margin.unwrap_or(DEFAULT_EXPIRATION_MARGIN),
                default_credential_expiration,
            )
        }
    }
}

#[derive(Clone)]
struct Provider<T: TimeSource> {
    time: T,
    cache: Cache,
    refresh: Arc<dyn AsyncProvideCredentials>,
    refresh_timeout: Duration,
    default_credential_expiration: Duration,
}

impl<T: TimeSource> Provider<T> {
    fn new(
        time: T,
        refresh: Arc<dyn AsyncProvideCredentials>,
        refresh_timeout: Duration,
        default_credential_expiration: Duration,
        expiration_margin: Duration,
    ) -> Self {
        Provider {
            time,
            cache: Cache::new(expiration_margin),
            refresh,
            refresh_timeout,
            default_credential_expiration,
        }
    }

    fn provide_credentials(&self) -> BoxFuture<CredentialsResult> {
        let now = self.time.now();
        let refresh = self.refresh.clone();
        let cache = self.cache.clone();
        let default_credential_expiration = self.default_credential_expiration;
        Box::pin(async move {
            if let Some(credentials) = cache.yield_or_clear_if_expired(now).await {
                Ok(credentials)
            } else {
                let span = trace_span!("lazy_refresh_credentials");
                let future = refresh.provide_credentials();
                cache
                    .refresh(|| {
                        async move {
                            let mut credentials = future.await?;
                            // If the credentials don't have an expiration time, then create a default one
                            if credentials.expiry().is_none() {
                                *credentials.expiry_mut() =
                                    Some(now + default_credential_expiration);
                            }
                            Ok(credentials)
                        }
                        .instrument(span)
                    })
                    .await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::provider::lazy_caching::{
        Provider, TimeSource, DEFAULT_CREDENTIAL_EXPIRATION, DEFAULT_EXPIRATION_MARGIN,
        DEFAULT_REFRESH_TIMEOUT,
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

    impl TimeSource for TestTime {
        fn now(&self) -> SystemTime {
            *self.time.lock().unwrap()
        }
    }

    fn test_provider<T: TimeSource>(time: T, refresh_list: Vec<CredentialsResult>) -> Provider<T> {
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
            DEFAULT_EXPIRATION_MARGIN,
        )
    }

    fn epoch_secs(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn credentials(expired_secs: u64) -> Credentials {
        Credentials::new("test", "test", None, Some(epoch_secs(expired_secs)), "test")
    }

    async fn expect_creds<T: TimeSource>(expired_secs: u64, provider: &Provider<T>) {
        let creds = provider
            .provide_credentials()
            .await
            .expect("expected credentials");
        assert_eq!(Some(epoch_secs(expired_secs)), creds.expiry());
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
            DEFAULT_EXPIRATION_MARGIN,
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
        provider.time.set(epoch_secs(1500));
        expect_creds(2000, &provider).await;
        expect_creds(2000, &provider).await;
        provider.time.set(epoch_secs(2500));
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
        provider.time.set(epoch_secs(1500));
        assert!(provider.provide_credentials().await.is_err());
    }

    #[test_env_log::test]
    fn refresh_retrieve_contention() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(16)
            .build()
            .unwrap();

        let provider = Arc::new(test_provider(
            TestTime::new(epoch_secs(0)),
            vec![
                Ok(credentials(500)),
                Ok(credentials(1500)),
                Ok(credentials(2500)),
                Ok(credentials(3500)),
                Ok(credentials(4500)),
            ],
        ));

        for i in 0..4 {
            let mut tasks = Vec::new();
            for j in 0..50 {
                let provider = provider.clone();
                tasks.push(rt.spawn(async move {
                    let now = epoch_secs(i * 1000 + (4 * j));
                    provider.time.set(now);

                    let creds = provider.provide_credentials().await.unwrap();
                    assert!(
                        creds.expiry().unwrap() >= now,
                        "{:?} >= {:?}",
                        creds.expiry(),
                        now
                    );
                }));
            }
            for task in tasks {
                rt.block_on(task).unwrap();
            }
        }
    }
}
