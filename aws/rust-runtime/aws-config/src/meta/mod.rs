/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Meta-providers that augment existing providers with new behavior

use std::borrow::Cow;
use std::error::Error;
use std::fmt;

pub mod credentials;
pub mod region;
pub mod token;

/// Error returned when all providers in a chain are exhausted without producing credentials or tokens.
///
/// Contains per-provider errors for programmatic inspection via [`ProviderChainError::attempts`].
///
/// To access from a `CredentialsError` or `TokenError`, downcast the error source:
/// ```ignore
/// use aws_config::meta::ProviderChainError;
/// use aws_credential_types::provider::error::CredentialsError;
/// use std::error::Error;
///
/// if let Some(chain_err) = err.source()
///     .and_then(|s| s.downcast_ref::<ProviderChainError<CredentialsError>>())
/// {
///     for (name, provider_err) in chain_err.attempts() {
///         println!("{name}: {provider_err}");
///     }
/// }
/// ```
#[derive(Debug)]
pub struct ProviderChainError<E: Error> {
    attempts: Vec<(Cow<'static, str>, E)>,
}

impl<E: Error> ProviderChainError<E> {
    pub(crate) fn new(attempts: Vec<(Cow<'static, str>, E)>) -> Self {
        Self { attempts }
    }

    /// Returns the per-provider errors in chain order.
    pub fn attempts(&self) -> &[(Cow<'static, str>, E)] {
        &self.attempts
    }
}

impl<E: Error> fmt::Display for ProviderChainError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.attempts.is_empty() {
            return write!(f, "no providers were configured in the chain");
        }
        write!(f, "no credentials found in chain. Attempted:")?;
        for (name, err) in &self.attempts {
            write!(f, "\n  {name}: {err}")?;
            let mut source = err.source();
            while let Some(cause) = source {
                write!(f, ": {cause}")?;
                source = cause.source();
            }
        }
        Ok(())
    }
}

impl<E: Error + 'static> Error for ProviderChainError<E> {}
