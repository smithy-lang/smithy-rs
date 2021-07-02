/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod env;

use crate::Credentials;
use smithy_http::property_bag::PropertyBag;
use std::error::Error;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::future::{self, Future};
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug)]
#[non_exhaustive]
pub enum CredentialsError {
    CredentialsNotLoaded,
    Unhandled(Box<dyn Error + Send + Sync + 'static>),
}

impl Display for CredentialsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CredentialsError::CredentialsNotLoaded => write!(f, "CredentialsNotLoaded"),
            CredentialsError::Unhandled(err) => write!(f, "{}", err),
        }
    }
}

impl Error for CredentialsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CredentialsError::Unhandled(e) => Some(e.as_ref() as _),
            _ => None,
        }
    }
}

pub type CredentialsResult = Result<Credentials, CredentialsError>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// An asynchronous credentials provider
///
/// If your use-case is synchronous, you should implement [ProvideCredentials] instead.
pub trait AsyncProvideCredentials: Send + Sync {
    fn provide_credentials(&self) -> BoxFuture<CredentialsResult>;
}

pub type CredentialsProvider = Arc<dyn AsyncProvideCredentials>;

/// A synchronous credentials provider
///
/// This is offered as a convenience for credential provider implementations that don't
/// need to be async. Otherwise, implement [AsyncProvideCredentials].
pub trait ProvideCredentials: Send + Sync {
    fn provide_credentials(&self) -> Result<Credentials, CredentialsError>;
}

impl<T> AsyncProvideCredentials for T
where
    T: ProvideCredentials,
{
    fn provide_credentials(&self) -> BoxFuture<CredentialsResult> {
        let result = self.provide_credentials();
        Box::pin(future::ready(result))
    }
}

pub fn default_provider() -> impl AsyncProvideCredentials {
    // TODO: this should be a chain based on the CRT
    env::EnvironmentVariableCredentialsProvider::new()
}

impl ProvideCredentials for Credentials {
    fn provide_credentials(&self) -> Result<Credentials, CredentialsError> {
        Ok(self.clone())
    }
}

pub fn set_provider(config: &mut PropertyBag, provider: Arc<dyn AsyncProvideCredentials>) {
    config.insert(provider);
}

#[cfg(test)]
mod test {
    use crate::Credentials;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn creds_are_send_sync() {
        assert_send_sync::<Credentials>()
    }
}
