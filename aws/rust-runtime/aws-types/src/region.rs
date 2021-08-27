/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::os_shim_internal::Env;
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// The region to send requests to.
///
/// The region MUST be specified on a request. It may be configured globally or on a
/// per-client basis unless otherwise noted. A full list of regions is found in the
/// "Regions and Endpoints" document.
///
/// See http://docs.aws.amazon.com/general/latest/gr/rande.html for
/// information on AWS regions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Region(
    // Regions are almost always known statically. However, as an escape hatch for when they
    // are not, allow for an owned region
    Cow<'static, str>,
);

impl AsRef<str> for Region {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Region {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Region {
    pub fn new(region: impl Into<Cow<'static, str>>) -> Self {
        Self(region.into())
    }
    pub const fn from_static(region: &'static str) -> Self {
        Self(Cow::Borrowed(region))
    }
}

pub struct ChainProvider {
    providers: Vec<Box<dyn ProvideRegion>>,
}

impl ChainProvider {
    pub async fn region(&self) -> Option<Region> {
        for provider in &self.providers {
            if let Some(region) = provider.region().await {
                return Some(region);
            }
        }
        None
    }
}

/// Implement a region provider based on a series of region providers
///
/// # Example
/// ```rust
/// use aws_types::region::{ChainProvider, Region};
/// use std::env;
/// // region provider that first checks the `CUSTOM_REGION` environment variable,
/// // then checks the default provider chain, then falls back to us-east-2
/// let provider = ChainProvider::first_try(env::var("CUSTOM_REGION").ok().map(Region::new))
///     .or_default_provider()
///     .or_else(Region::new("us-east-2"));
/// ```
impl ChainProvider {
    pub fn first_try(provider: impl ProvideRegion + 'static) -> Self {
        ChainProvider {
            providers: vec![Box::new(provider)],
        }
    }
    pub fn or_else(mut self, fallback: impl ProvideRegion + 'static) -> Self {
        self.providers.push(Box::new(fallback));
        self
    }

    pub fn or_default_provider(mut self) -> Self {
        self.providers.push(Box::new(default_provider()));
        self
    }
}

impl ProvideRegion for Option<Region> {
    fn region(&self) -> RegionFuture {
        RegionFuture::ready(self.clone())
    }
}

impl ProvideRegion for ChainProvider {
    fn region(&self) -> RegionFuture {
        RegionFuture::new(self.region())
    }
}

pub struct RegionFuture<'a>(Pin<Box<dyn Future<Output = Option<Region>> + Send + 'a>>);

impl<'a> RegionFuture<'a> {
    pub fn new(f: impl Future<Output = Option<Region>> + Send + 'a) -> Self {
        RegionFuture(Box::pin(f))
    }

    pub fn ready(region: Option<Region>) -> Self {
        Self::new(std::future::ready(region))
    }
}

impl Future for RegionFuture<'_> {
    type Output = Option<Region>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

/// Provide a [`Region`](Region) to use with AWS requests
///
/// For most cases [`default_provider`](default_provider) will be the best option, implementing
/// a standard provider chain.
pub trait ProvideRegion: Send + Sync {
    fn region(&self) -> RegionFuture;
}

impl ProvideRegion for Region {
    fn region(&self) -> RegionFuture {
        RegionFuture::ready(Some(self.clone()))
    }
}

impl<'a> ProvideRegion for &'a Region {
    fn region(&self) -> RegionFuture {
        RegionFuture::ready(Some((*self).clone()))
    }
}

pub fn default_provider() -> impl ProvideRegion {
    EnvironmentProvider::new()
}

#[non_exhaustive]
pub struct EnvironmentProvider {
    env: Env,
}

impl Default for EnvironmentProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::redundant_closure)] // https://github.com/rust-lang/rust-clippy/issues/7218
impl EnvironmentProvider {
    pub fn new() -> Self {
        EnvironmentProvider { env: Env::real() }
    }
}

impl ProvideRegion for EnvironmentProvider {
    fn region(&self) -> RegionFuture {
        RegionFuture::ready(
            self.env
                .get("AWS_REGION")
                .or_else(|_| self.env.get("AWS_DEFAULT_REGION"))
                .map(Region::new)
                .ok(),
        )
    }
}

/// The region to use when signing requests
///
/// Generally, user code will not need to interact with `SigningRegion`. See `[Region](crate::Region)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningRegion(Cow<'static, str>);

impl AsRef<str> for SigningRegion {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Region> for SigningRegion {
    fn from(inp: Region) -> Self {
        SigningRegion(inp.0)
    }
}

impl SigningRegion {
    pub fn from_static(region: &'static str) -> Self {
        SigningRegion(Cow::Borrowed(region))
    }
}
