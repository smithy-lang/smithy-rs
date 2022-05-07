//! Accept MFA tokens from user input when assuming a role
//!

use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use crate::profile::mfa_token::MfaTokenFetchError::ProviderError;

/// The value provided by the user's MFA device, if required to authenticate the session
pub struct MfaToken(pub(crate) String);

/// An error fetching MFA token input from the user or MFA token source
#[derive(Debug)]
#[non_exhaustive]
pub enum MfaTokenFetchError {
    /// MFA token is required to assume the requested role, but no MFA token provider has been configured.
    NoMfaTokenProviderConfigured,

    /// The provider experienced an error while fetching credentials from the user.
    ///
    /// This may include errors like the user closing a dialog box that requests an MFA token
    #[non_exhaustive]
    ProviderError {
        /// Underlying cause of the error
        cause: Box<dyn Error + Send + Sync + 'static>,
    },
}

impl MfaTokenFetchError {
    /// The MFA token provider returned an error
    ///
    /// This may include errors like the user closing a dialog box that requests an MFA token
    pub fn provider_error(cause: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
        ProviderError { cause: cause.into() }
    }
}


impl Display for MfaTokenFetchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MfaTokenFetchError::NoMfaTokenProviderConfigured => write!(
                f,
                "An MFA token was requested but no provider was configured"
            ),
            ProviderError { cause } => write!(
                f,
                "An error occurred while fetching an MFA token: {}",
                cause
            ),
        }
    }
}

impl Error for MfaTokenFetchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            MfaTokenFetchError::NoMfaTokenProviderConfigured => None,
            ProviderError { cause } => Some(cause.as_ref()),
        }
    }
}

/// Result type for MFA token providers
pub type Result = std::result::Result<MfaToken, MfaTokenFetchError>;

impl<T: Into<String>> From<T> for MfaToken {
    fn from(s: T) -> Self {
        MfaToken(s.into())
    }
}


/// Future wrapper returned by [`ProvideMfaToken`]
///
/// Note: this module should only be used when implementing your own mfa token providers.
pub mod future {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use aws_smithy_async::future::now_or_later::NowOrLater;

    /// Future returned by [`ProvideMfaToken`](super::ProvideMfaToken)
    ///
    /// - When wrapping an already loaded mfa_token, use [`ready`](MfaToken::ready).
    /// - When wrapping an asynchronously loaded mfa_token, use [`new`](MfaToken::new).
    pub struct ProvideMfaToken<'a>(NowOrLater<super::Result, Pin<Box<dyn Future<Output=super::Result> + Send + 'a>>>);

    impl<'a> ProvideMfaToken<'a> {
        /// Creates a `ProvideMfaToken` from a resolved MFA token
        pub fn ready(mfa_token: super::Result) -> Self {
            ProvideMfaToken(NowOrLater::ready(mfa_token))
        }

        /// Creates a `ProvideMfaToken` from future that will resolve to an MFA token
        pub fn new(future: impl Future<Output=super::Result> + Send + 'a) -> Self {
            ProvideMfaToken(NowOrLater::new(Box::pin(future)))
        }
    }

    impl<'a> Future for ProvideMfaToken<'a> {
        type Output = super::Result;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx)
        }
    }
}

/// Request an MFA token from the user as part of authentication flow.
///
/// Implementations are expected to prompt the user to provide a token
/// from their MFA device.
pub trait ProvideMfaToken: Send + Sync + Debug {
    /// Request an MFA token from the user
    ///
    /// `mfa_serial` specified the serial number of the MFA token in use,
    /// and may be displayed to the user in order for them to identify
    /// which MFA device they should use.
    fn mfa_token(&self, mfa_serial: &str) -> future::ProvideMfaToken;
}

/// The default implementation will fail to resolve MFA tokens during authentication
///
/// Application authors must provide their own `ProvideMfaToken` implementation if they
/// wish to source MFA tokens from their users.
#[derive(Debug, Default)]
pub(crate) struct DefaultProvideNoMfaToken {}

impl ProvideMfaToken for DefaultProvideNoMfaToken {
    fn mfa_token(&self, mfa_serial: &str) -> future::ProvideMfaToken {
        tracing::info!(mfa_serial=%mfa_serial, "MFA code is required as mfa_serial was set, but no MFA token provider has been configured");
        future::ProvideMfaToken::ready(Err(MfaTokenFetchError::NoMfaTokenProviderConfigured))
    }
}
