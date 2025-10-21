/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credentials from an AWS Console session vended by AWS Sign-In.

mod cache;
mod token;

use crate::provider_config::ProviderConfig;
use aws_credential_types::provider::future;
use aws_credential_types::provider::ProvideCredentials;
use aws_types::os_shim_internal::{Env, Fs};

// TODO(sign-in): fill in additional details on this provider, examples, and links to documentation

/// AWS credentials provider vended by AWS Sign-In. This provider allows users to acquire AWS
/// credentials that correspond to an AWS Console session.
#[derive(Debug)]
pub struct SignInCredentialProvider {
    fs: Fs,
    env: Env,
    session_arn: String,
    // sdk_config: SdkConfig,
    // time_source: SharedTimeSource,
}

impl SignInCredentialProvider {
    /// Create a new [`SignInCredentialProviderBuilder`] for the given login session ARN.
    ///
    /// The `session_arn` argument should take the form an Amazon Resource Name (ARN) like
    ///
    /// ```text
    /// arn:aws:iam::0123456789012:user/Admin
    /// ```
    pub fn builder(session_arn: impl Into<String>) -> SignInCredentialProviderBuilder {
        SignInCredentialProviderBuilder {
            session_arn: session_arn.into(),
            provider_config: None,
        }
    }
}

impl ProvideCredentials for SignInCredentialProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        todo!()
    }
}

/// Builder for [`SignInCredentialProvider`]
#[derive(Debug)]
pub struct SignInCredentialProviderBuilder {
    session_arn: String,
    provider_config: Option<ProviderConfig>,
}

impl SignInCredentialProviderBuilder {
    /// Override the configuration used for this provider
    pub fn configure(mut self, provider_config: &ProviderConfig) -> Self {
        self.provider_config = Some(provider_config.clone());
        self
    }

    /// Construct a SignInCredentialsProvider from the builder
    pub fn build(self) -> SignInCredentialProvider {
        let provider_config = self.provider_config.unwrap_or_default();
        let fs = provider_config.fs();
        let env = provider_config.env();
        SignInCredentialProvider {
            fs,
            env,
            session_arn: self.session_arn,
            // sdk_config: self.provider_config.unwrap_or_default().sdk_config.unwrap_or_default(),
            // time_source: self.provider_config.unwrap_or_default().time_source.unwrap_or_default(),
        }
    }
}
