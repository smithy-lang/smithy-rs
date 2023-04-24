/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Providers for AWS identity types.

pub mod credentials;
pub mod token;

// TODO(enableNewSmithyRuntime): Delete these deprecated re-exports
#[deprecated(note = "use `aws_credential_types::credentials::error` instead")]
pub use credentials::error;
#[deprecated(note = "use `aws_credential_types::credentials::future` instead")]
pub use credentials::future;
#[deprecated(note = "use `aws_credential_types::credentials::ProvideCredentials` instead")]
pub use credentials::ProvideCredentials;
#[deprecated(note = "use `aws_credential_types::credentials::Result` instead")]
pub use credentials::Result;
#[deprecated(
    note = "use `aws_credential_types::credentials::SharedCredentialsProvider` instead"
)]
pub use credentials::SharedCredentialsProvider;
