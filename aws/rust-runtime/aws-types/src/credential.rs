/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! AWS SDK Credentials
//!
//! ## Implementing your own credentials provider
//!
//! While for many use cases, using a built in credentials provider is sufficient, you may want to
//! implement your own credential provider.
//!
//! ### With static credentials
//! [`Credentials`](credentials::Credentials) implement
//! [`ProvideCredentials](provide_credentials::ProvideCredentials) directly, so no custom provider
//! implementation is required:
//! ```rust
//! use aws_types::Credentials;
//! # mod dynamodb {
//! # use aws_types::credential::ProvideCredentials;
//! # pub struct Config;
//! # impl Config {
//! #    pub fn builder() -> Self {
//! #        Config
//! #    }
//! #    pub fn credentials_provider(self, provider: impl ProvideCredentials + 'static) -> Self {
//! #       self
//! #    }
//! # }
//! # }
//!
//! let my_creds = Credentials::from_keys("akid", "secret_key", None);
//! let conf = dynamodb::Config::builder().credentials_provider(my_creds);
//! ```
//! ### With dynamically loaded credentials
//! If you are loading credentials dynamically, you can provide your own implementation of
//! [`ProvideCredentials`](provide_credentials::ProvideCredentials). Generally, this is best done by
//! defining an inherent `async fn` on your structure, then calling that method directly from
//! the trait implementation.
//! ```rust
//! use aws_types::credential::{CredentialsError, provide_credentials, Credentials};
//! use aws_types::credential::provide_credentials::future::ProvideCredentials;
//! struct SubprocessCredentialProvider;
//!
//! async fn invoke_command(command: &str) -> String {
//!     // implementation elided...
//!     # String::from("some credentials")
//! }
//!
//! /// Parse access key and secret from the first two lines of a string
//! fn parse_credentials(creds: &str) -> provide_credentials::Result {
//!     let mut lines = creds.lines();
//!     let akid = lines.next().ok_or(CredentialsError::ProviderError("invalid credentials".into()))?;
//!     let secret = lines.next().ok_or(CredentialsError::ProviderError("invalid credentials".into()))?;
//!     Ok(Credentials::new(akid, secret, None, None, "CustomCommand"))
//! }
//!
//! impl SubprocessCredentialProvider {
//!     async fn load_credentials(&self) -> provide_credentials::Result {
//!         let creds = invoke_command("load-credentials.py").await;
//!         parse_credentials(&creds)
//!     }
//! }
//!
//! impl provide_credentials::ProvideCredentials for SubprocessCredentialProvider {
//!     fn provide_credentials<'a>(&'a self) -> ProvideCredentials<'a> where Self: 'a {
//!         ProvideCredentials::new(self.load_credentials())
//!     }
//! }
//! ```

pub mod credentials;
pub mod provide_credentials;

pub use credentials::Credentials;
pub use provide_credentials::CredentialsError;
pub use provide_credentials::ProvideCredentials;
pub use provide_credentials::Result;
