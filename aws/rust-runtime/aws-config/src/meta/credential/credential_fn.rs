/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::credential;
use aws_types::credential::{provide_credentials, ProvideCredentials};
use std::future::Future;
use std::marker::PhantomData;

/// A [`ProvideCredentials`] implemented by a closure.
///
/// See [`async_provide_credentials_fn`] for more details.
#[derive(Copy, Clone)]
pub struct ProvideCredentialsFn<'c, T, F>
where
    T: Fn() -> F + Send + Sync + 'c,
    F: Future<Output = credential::Result> + Send + 'static,
{
    f: T,
    phantom: PhantomData<&'c T>,
}

impl<'c, T, F> ProvideCredentials for ProvideCredentialsFn<'c, T, F>
where
    T: Fn() -> F + Send + Sync + 'c,
    F: Future<Output = credential::Result> + Send + 'static,
{
    fn provide_credentials<'a>(&'a self) -> provide_credentials::future::ProvideCredentials
    where
        Self: 'a,
    {
        provide_credentials::future::ProvideCredentials::new((self.f)())
    }
}

/// Returns a new [`ProvideCredentialsFn`] with the given closure. This allows you
/// to create an [`ProvideCredentials`] implementation from an async block that returns
/// a [`credentials::Result`].
///
/// # Example
///
/// ```
/// use aws_types::Credentials;
/// use aws_config::meta::credential::credential_fn::async_provide_credentials_fn;
///
/// async fn load_credentials() -> Credentials {
///     todo!()
/// }
///
/// async_provide_credentials_fn(|| async {
///     // Async process to retrieve credentials goes here
///     let credentials = load_credentials().await;
///     Ok(credentials)
/// });
/// ```
pub fn async_provide_credentials_fn<'c, T, F>(f: T) -> ProvideCredentialsFn<'c, T, F>
where
    T: Fn() -> F + Send + Sync + 'c,
    F: Future<Output = credential::Result> + Send + 'static,
{
    ProvideCredentialsFn {
        f,
        phantom: Default::default(),
    }
}
