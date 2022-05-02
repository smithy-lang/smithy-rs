use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use crate::profile::mfa_token::MfaTokenFetchError::ProviderError;

pub struct MfaToken(pub(crate) String);

#[derive(Debug)]
#[non_exhaustive]
pub enum MfaTokenFetchError {
    NoMfaTokenProviderConfigured,
    ProviderError {
        cause: Box<dyn Error + Send + Sync + 'static>,
    },
}

impl MfaTokenFetchError {
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
            MfaTokenFetchError::ProviderError { cause } => write!(
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
            MfaTokenFetchError::ProviderError { cause } => Some(cause.as_ref()),
            _ => None
        }
    }
}

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
        pub fn ready(mfa_token: super::Result) -> Self {
            ProvideMfaToken(NowOrLater::ready(mfa_token))
        }

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

pub trait ProvideMfaToken: Send + Sync + Debug {
    fn mfa_token(&self, mfa_serial: &str) -> future::ProvideMfaToken;
}

#[derive(Debug, Default)]
pub(crate) struct DefaultProvideNoMfaToken {}

impl ProvideMfaToken for DefaultProvideNoMfaToken {
    fn mfa_token(&self, mfa_serial: &str) -> future::ProvideMfaToken {
        tracing::info!(mfa_serial=%mfa_serial, "MFA code is required as mfa_serial was set, but no MFA token provider has been configured");
        future::ProvideMfaToken::ready(Err(MfaTokenFetchError::NoMfaTokenProviderConfigured))
    }
}
