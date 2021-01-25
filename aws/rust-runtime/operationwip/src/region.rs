/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use smithy_http::property_bag::PropertyBag;

#[derive(Clone)]
pub struct Region(String);

impl AsRef<str> for Region {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl Region {
    pub fn new(region: impl Into<String>) -> Self {
        Region(region.into())
    }
}

pub trait ProvideRegion {
    fn region(&self) -> Option<Region>;
}

impl ProvideRegion for &str {
    fn region(&self) -> Option<Region> {
        Some(Region((*self).into()))
    }
}

struct RegionEnvironment;

impl ProvideRegion for RegionEnvironment {
    fn region(&self) -> Option<Region> {
        std::env::var("AWS_DEFAULT_REGION").map(Region::new).ok()
    }
}

pub fn default_provider() -> impl ProvideRegion {
    RegionEnvironment
}

pub struct SigningRegion(String);

impl SigningRegion {
    pub fn new(region: impl Into<String>) -> Self {
        SigningRegion(region.into())
    }
}

pub trait RegionExt {
    fn request_region(&self) -> Option<&str>;
    fn signing_region(&self) -> Option<&str>;
}

impl RegionExt for PropertyBag {
    fn request_region(&self) -> Option<&str> {
        self.get::<Region>().map(|reg| reg.0.as_str())
    }

    fn signing_region(&self) -> Option<&str> {
        self.get::<SigningRegion>()
            .map(|reg| reg.0.as_str())
            .or_else(|| self.request_region())
    }
}

#[cfg(test)]
mod test {
    use crate::extensions::Extensions;
    use crate::region::{Region, RegionExt, SigningRegion};

    #[test]
    fn signing_region_fallback() {
        let mut extensions = Extensions::new();
        extensions.insert(Region::new("aws-global"));
        assert_eq!(extensions.signing_region(), Some("aws-global"));

        extensions.insert(SigningRegion::new("us-east-1"));
        assert_eq!(extensions.signing_region(), Some("us-east-1"))
    }
}
