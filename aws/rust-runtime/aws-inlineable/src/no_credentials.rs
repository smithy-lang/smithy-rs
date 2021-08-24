/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::credential::provide_credentials::future;
use aws_types::credential::{CredentialsError, ProvideCredentials};

/// Stub Provider for use when no credentials provider is used
#[non_exhaustive]
pub struct NoCredentials;

impl ProvideCredentials for NoCredentials {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::ready(Err(CredentialsError::CredentialsNotLoaded))
    }
}
