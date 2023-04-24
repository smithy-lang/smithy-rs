/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AWS SDK Access Tokens

use crate::AccessToken;
use std::sync::Arc;

/// Token provider errors
pub mod error {
    use std::error::Error;
    use std::fmt;
    use std::time::Duration;

    /// Details for [`AccessTokenError::TokenNotLoaded`]
    #[derive(Debug)]
    pub struct TokenNotLoaded {
        source: Box<dyn Error + Send + Sync + 'static>,
    }

    /// Details for [`AccessTokenError::ProviderTimedOut`]
    #[derive(Debug)]
    pub struct ProviderTimedOut {
        timeout_duration: Duration,
    }

    impl ProviderTimedOut {
        /// Returns the maximum allowed timeout duration that was exceeded
        pub fn timeout_duration(&self) -> Duration {
            self.timeout_duration
        }
    }

    /// Details for [`AccessTokenError::InvalidConfiguration`]
    #[derive(Debug)]
    pub struct InvalidConfiguration {
        source: Box<dyn Error + Send + Sync + 'static>,
    }

    /// Details for [`AccessTokenError::ProviderError`]
    #[derive(Debug)]
    pub struct ProviderError {
        source: Box<dyn Error + Send + Sync + 'static>,
    }

    /// Details for [`AccessTokenError::Unhandled`]
    #[derive(Debug)]
    pub struct Unhandled {
        source: Box<dyn Error + Send + Sync + 'static>,
    }

    /// Error returned when an access token provider fails to provide an access token.
    #[derive(Debug)]
    pub enum AccessTokenError {
        /// This provider couldn't provide a token.
        TokenNotLoaded(TokenNotLoaded),

        /// Loading a token from this provider exceeded the maximum allowed time.
        ProviderTimedOut(ProviderTimedOut),

        /// The provider was given invalid configuration.
        ///
        /// For example, a syntax error in `~/.aws/config`.
        InvalidConfiguration(InvalidConfiguration),

        /// The provider experienced an error during credential resolution.
        ProviderError(ProviderError),

        /// An unexpected error occurred during token resolution.
        ///
        /// If the error is something that can occur during expected usage of a provider, `ProviderError`
        /// should be returned instead. Unhandled is reserved for exceptional cases, for example:
        /// - Returned data not UTF-8
        /// - A provider returns data that is missing required fields
        Unhandled(Unhandled),
    }

    impl AccessTokenError {
        /// The access token provider couldn't provide a token.
        ///
        /// This error indicates the token provider was not enable or no configuration was set.
        /// This contrasts with [`invalid_configuration`](AccessTokenError::InvalidConfiguration), indicating
        /// that the provider was configured in some way, but certain settings were invalid.
        pub fn not_loaded(source: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
            AccessTokenError::TokenNotLoaded(TokenNotLoaded {
                source: source.into(),
            })
        }

        /// An unexpected error occurred loading an access token from this provider.
        ///
        /// Unhandled errors should not occur during normal operation and should be reserved for exceptional
        /// cases, such as a JSON API returning an output that was not parseable as JSON.
        pub fn unhandled(source: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
            Self::Unhandled(Unhandled {
                source: source.into(),
            })
        }

        /// The access token provider returned an error.
        pub fn provider_error(source: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
            Self::ProviderError(ProviderError {
                source: source.into(),
            })
        }

        /// The provided configuration for a provider was invalid.
        pub fn invalid_configuration(
            source: impl Into<Box<dyn Error + Send + Sync + 'static>>,
        ) -> Self {
            Self::InvalidConfiguration(InvalidConfiguration {
                source: source.into(),
            })
        }

        /// The access token provider did not provide a token within an allotted amount of time.
        pub fn provider_timed_out(timeout_duration: Duration) -> Self {
            Self::ProviderTimedOut(ProviderTimedOut { timeout_duration })
        }
    }

    impl fmt::Display for AccessTokenError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                AccessTokenError::TokenNotLoaded(_) => {
                    write!(f, "the access token provider was not enabled")
                }
                AccessTokenError::ProviderTimedOut(details) => write!(
                    f,
                    "access token provider timed out after {} seconds",
                    details.timeout_duration.as_secs()
                ),
                AccessTokenError::InvalidConfiguration(_) => {
                    write!(f, "the access token provider was not properly configured")
                }
                AccessTokenError::ProviderError(_) => {
                    write!(f, "an error occurred while loading an access token")
                }
                AccessTokenError::Unhandled(_) => {
                    write!(f, "unexpected access token providererror")
                }
            }
        }
    }

    impl Error for AccessTokenError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                AccessTokenError::TokenNotLoaded(details) => Some(details.source.as_ref() as _),
                AccessTokenError::ProviderTimedOut(_) => None,
                AccessTokenError::InvalidConfiguration(details) => {
                    Some(details.source.as_ref() as _)
                }
                AccessTokenError::ProviderError(details) => Some(details.source.as_ref() as _),
                AccessTokenError::Unhandled(details) => Some(details.source.as_ref() as _),
            }
        }
    }
}

/// Result type for token providers
pub type Result = std::result::Result<AccessToken, error::AccessTokenError>;

/// Convenience `ProvideToken` struct that implements the `ProvideToken` trait.
pub mod future {
    use aws_smithy_async::future::now_or_later::NowOrLater;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    /// Future new-type that `ProvideAccessToken::provide_access_token` must return.
    #[derive(Debug)]
    pub struct ProvideAccessToken<'a>(NowOrLater<super::Result, BoxFuture<'a, super::Result>>);

    impl<'a> ProvideAccessToken<'a> {
        /// Creates a `ProvideAccessToken` struct from a future.
        pub fn new(future: impl Future<Output = super::Result> + Send + 'a) -> Self {
            ProvideAccessToken(NowOrLater::new(Box::pin(future)))
        }

        /// Creates a `ProvideAccessToken` struct from a resolved credentials value.
        pub fn ready(credentials: super::Result) -> Self {
            ProvideAccessToken(NowOrLater::ready(credentials))
        }
    }

    impl Future for ProvideAccessToken<'_> {
        type Output = super::Result;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx)
        }
    }
}

/// Access Token Provider
pub trait ProvideAccessToken: Send + Sync + std::fmt::Debug {
    /// Returns a future that provides an access token.
    fn provide_access_token<'a>(&'a self) -> future::ProvideAccessToken<'a>
    where
        Self: 'a;
}

/// Access token provider wrapper that may be shared.
///
/// Newtype wrapper around [`ProvideAccessToken`] that implements `Clone` using an internal `Arc`.
#[derive(Clone, Debug)]
pub struct SharedAccessTokenProvider(Arc<dyn ProvideAccessToken>);

impl SharedAccessTokenProvider {
    /// Create a new [`SharedAccessTokenProvider`] from [`ProvideAccessToken`].
    ///
    /// The given provider will be wrapped in an internal `Arc`. If your
    /// provider is already in an `Arc`, use `SharedAccessTokenProvider::from(provider)` instead.
    pub fn new(provider: impl ProvideAccessToken + 'static) -> Self {
        Self(Arc::new(provider))
    }
}

impl AsRef<dyn ProvideAccessToken> for SharedAccessTokenProvider {
    fn as_ref(&self) -> &(dyn ProvideAccessToken + 'static) {
        self.0.as_ref()
    }
}

impl From<Arc<dyn ProvideAccessToken>> for SharedAccessTokenProvider {
    fn from(provider: Arc<dyn ProvideAccessToken>) -> Self {
        SharedAccessTokenProvider(provider)
    }
}

impl ProvideAccessToken for SharedAccessTokenProvider {
    fn provide_access_token<'a>(&'a self) -> future::ProvideAccessToken<'a>
    where
        Self: 'a,
    {
        self.0.provide_access_token()
    }
}
