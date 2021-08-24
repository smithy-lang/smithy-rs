/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::Region;

pub struct ProviderChain {
    providers: Vec<Box<dyn ProvideRegion>>,
}

impl ProviderChain {
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
/// use aws_types::region::Region;
/// use std::env;
/// use aws_config::meta::region::ProviderChain;
/// // region provider that first checks the `CUSTOM_REGION` environment variable,
/// // then checks the default provider chain, then falls back to us-east-2
/// let provider = ProviderChain::first_try(env::var("CUSTOM_REGION").ok().map(Region::new))
///     .or_default_provider()
///     .or_else(Region::new("us-east-2"));
/// ```
impl ProviderChain {
    pub fn first_try(provider: impl ProvideRegion + 'static) -> Self {
        ProviderChain {
            providers: vec![Box::new(provider)],
        }
    }
    pub fn or_else(mut self, fallback: impl ProvideRegion + 'static) -> Self {
        self.providers.push(Box::new(fallback));
        self
    }

    #[cfg(feature = "default-provider")]
    pub fn or_default_provider(mut self) -> Self {
        self.providers
            .push(Box::new(crate::default_provider::region::default_provider()));
        self
    }
}

impl ProvideRegion for Option<Region> {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::ready(self.clone())
    }
}

impl ProvideRegion for ProviderChain {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::new(self.region())
    }
}

pub mod future {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use smithy_async::future::now_or_later::NowOrLater;

    use aws_types::region::Region;

    type BoxFuture<'a> = Pin<Box<dyn Future<Output = Option<Region>> + Send + 'a>>;
    pub struct ProvideRegion<'a>(NowOrLater<Option<Region>, BoxFuture<'a>>);
    impl<'a> ProvideRegion<'a> {
        pub fn new(f: impl Future<Output = Option<Region>> + Send + 'a) -> Self {
            Self(NowOrLater::new(Box::pin(f)))
        }

        pub fn ready(region: Option<Region>) -> Self {
            Self(NowOrLater::ready(region))
        }
    }

    impl Future for ProvideRegion<'_> {
        type Output = Option<Region>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.0).poll(cx)
        }
    }
}

/// Provide a [`Region`](Region) to use with AWS requests
///
/// For most cases [`default_provider`](default_provider) will be the best option, implementing
/// a standard provider chain.
pub trait ProvideRegion: Send + Sync {
    fn region(&self) -> future::ProvideRegion;
}

impl ProvideRegion for Region {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::ready(Some(self.clone()))
    }
}

impl<'a> ProvideRegion for &'a Region {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::ready(Some((*self).clone()))
    }
}
