/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Note: This code originally lived in the `aws-runtime` crate. It was moved here to avoid circular dependencies
/// This module is re-exported in `aws-runtime`, and so even though this is a pre-1.0 crate, this module should not
/// have any breaking changes
use aws_smithy_types::config_bag::{Storable, StoreAppend};

/// IDs for the features that may be used in the AWS SDK
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AwsSdkFeature {
    /// Indicates that an operation was called by the S3 Transfer Manager
    S3Transfer,
    // Various features related to how Credentials are set
    /// An operation called using credentials resolved from code, cli parameters, session object, or client instance
    CredentialsCode,
    /// An operation called using credentials resolved from environment variables
    CredentialsEnvVars,
    /// An operation called using credentials resolved from environment variables for assuming a role with STS using a web identity token
    CredentialsEnvVarsStsWebIdToken,
    /// An operation called using credentials resolved from STS using assume role
    CredentialsStsAssumeRole,
    /// An operation called using credentials resolved from STS using assume role with SAML
    CredentialsStsAssumeRoleSaml,
    /// An operation called using credentials resolved from STS using assume role with web identity
    CredentialsStsAssumeRoleWebId,
    /// An operation called using credentials resolved from STS using a federation token
    CredentialsStsFederationToken,
    /// An operation called using credentials resolved from STS using a session token
    CredentialsStsSessionToken,
    /// An operation called using credentials resolved from a config file(s) profile with static credentials
    CredentialsProfile,
    /// An operation called using credentials resolved from a source profile in a config file(s) profile
    CredentialsProfileSourceProfile,
    /// An operation called using credentials resolved from a named provider in a config file(s) profile
    CredentialsProfileNamedProvider,
    /// An operation called using credentials resolved from configuration for assuming a role with STS using web identity token in a config file(s) profile
    CredentialsProfileStsWebIdToken,
    /// An operation called using credentials resolved from an SSO session in a config file(s) profile
    CredentialsProfileSso,
    /// An operation called using credentials resolved from an SSO session
    CredentialsSso,
    /// An operation called using credentials resolved from a process in a config file(s) profile
    CredentialsProfileProcess,
    /// An operation called using credentials resolved from a process
    CredentialsProcess,
    /// An operation called using credentials resolved from an HTTP endpoint
    CredentialsHttp,
    /// An operation called using credentials resolved from the instance metadata service (IMDS)
    CredentialsImds,
}

impl Storable for AwsSdkFeature {
    type Storer = StoreAppend<Self>;
}
