/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! RetryConfig providers that augment existing providers with new functionality

use smithy_types::retry::RetryConfig;
use std::fmt::Debug;
use tracing::Instrument;

/// Load a retry_config by selecting the first from a series of retry_config providers.
///
/// # Examples
/// ```rust
/// use smithy_types::retry::{RetryMode, RetryConfig};
/// use std::env;
/// use aws_config::meta::retry_config::RetryConfigProviderChain;
/// // retry_config provider that first checks the `CUSTOM_REGION` environment variable,
/// // then checks the default provider chain, then falls back to standard
/// let provider = RetryConfigProviderChain::first_try(
///         env::var("AWS_RETRY_MODE")
///         .ok()
///         .map(|retry_mode| RetryConfig::from_str(&retry_mode))
///     )
///     .or_default_provider()
///     .or_else(RetryConfig::new().with_retry_mode(RetryMode::Standard));
/// ```
#[derive(Debug)]
pub struct RetryConfigProviderChain {
    providers: Vec<Box<dyn ProvideRetryConfig>>,
}

impl RetryConfigProviderChain {
    /// Load a retry_config from the provider chain
    ///
    /// The first provider to return a non-optional retry_config will be selected
    pub async fn retry_config(&self) -> Option<RetryConfig> {
        for provider in &self.providers {
            if let Some(retry_config) = provider
                .retry_config()
                .instrument(tracing::info_span!("load_retry_config", provider = ?provider))
                .await
            {
                return Some(retry_config);
            }
        }
        None
    }

    /// Create a default provider chain that starts by checking this provider.
    pub fn first_try(provider: impl ProvideRetryConfig + 'static) -> Self {
        RetryConfigProviderChain {
            providers: vec![Box::new(provider)],
        }
    }

    /// Add a fallback provider to the retry_config provider chain.
    pub fn or_else(mut self, fallback: impl ProvideRetryConfig + 'static) -> Self {
        self.providers.push(Box::new(fallback));
        self
    }

    /// Create a retry_config provider chain that starts by checking the default provider.
    #[cfg(feature = "default-provider")]
    pub fn default_provider() -> Self {
        Self::first_try(crate::default_provider::retry_config::default_provider())
    }

    /// Fallback to the default provider
    #[cfg(feature = "default-provider")]
    pub fn or_default_provider(mut self) -> Self {
        self.providers.push(Box::new(
            crate::default_provider::retry_config::default_provider(),
        ));
        self
    }
}

impl ProvideRetryConfig for Option<RetryConfig> {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        future::ProvideRetryConfig::ready(self.clone())
    }
}

impl ProvideRetryConfig for RetryConfigProviderChain {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        future::ProvideRetryConfig::new(RetryConfigProviderChain::retry_config(self))
    }
}

/// Future wrapper returned by [`ProvideRetryConfig`]
///
/// Note: this module should only be used when implementing your own retry_config providers.
pub mod future {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use smithy_async::future::now_or_later::NowOrLater;

    use smithy_types::retry::RetryConfig;

    type BoxFuture<'a> = Pin<Box<dyn Future<Output = Option<RetryConfig>> + Send + 'a>>;
    /// Future returned by [`ProvideRetryConfig`](super::ProvideRetryConfig)
    ///
    /// - When wrapping an already loaded retry_config, use [`ready`](ProvideRetryConfig::ready).
    /// - When wrapping an asynchronously loaded retry_config, use [`new`](ProvideRetryConfig::new).
    pub struct ProvideRetryConfig<'a>(NowOrLater<Option<RetryConfig>, BoxFuture<'a>>);
    impl<'a> ProvideRetryConfig<'a> {
        /// A future that wraps the given future
        pub fn new(future: impl Future<Output = Option<RetryConfig>> + Send + 'a) -> Self {
            Self(NowOrLater::new(Box::pin(future)))
        }

        /// A future that resolves to a given retry_config
        pub fn ready(retry_config: Option<RetryConfig>) -> Self {
            Self(NowOrLater::ready(retry_config))
        }
    }

    impl Future for ProvideRetryConfig<'_> {
        type Output = Option<RetryConfig>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx)
        }
    }
}

/// Provide a [`RetryConfig`](RetryConfig) to configure retry behavior when making AWS requests
///
/// For most cases [`default_provider`](crate::default_provider::retry_config::default_provider) will be the best option, implementing
/// a standard provider chain.
pub trait ProvideRetryConfig: Send + Sync + Debug {
    /// Load a retry_config from this provider
    fn retry_config(&self) -> future::ProvideRetryConfig;
}

impl ProvideRetryConfig for RetryConfig {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        future::ProvideRetryConfig::ready(Some(self.clone()))
    }
}

impl<'a> ProvideRetryConfig for &'a RetryConfig {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        future::ProvideRetryConfig::ready(Some((*self).clone()))
    }
}

impl ProvideRetryConfig for Box<dyn ProvideRetryConfig> {
    fn retry_config(&self) -> future::ProvideRetryConfig {
        self.as_ref().retry_config()
    }
}

#[cfg(test)]
mod test {
    use crate::meta::retry_config::RetryConfigProviderChain;
    use futures_util::FutureExt;
    use smithy_types::retry::RetryConfig;

    #[test]
    fn provider_chain() {
        let a = None;
        let b = Some(RetryConfig::new());
        let chain = RetryConfigProviderChain::first_try(a).or_else(b);
        assert_eq!(
            chain.retry_config().now_or_never().expect("ready"),
            Some(RetryConfig::new())
        );
    }

    #[test]
    fn empty_chain() {
        let chain = RetryConfigProviderChain::first_try(None).or_else(None);
        assert_eq!(chain.retry_config().now_or_never().expect("ready"), None);
    }
}
