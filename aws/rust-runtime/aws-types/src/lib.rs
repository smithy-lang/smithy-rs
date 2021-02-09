use std::borrow::Cow;
use std::sync::Arc;

/// The region to send requests to.
///
/// The region MUST be specified on a request. It may be configured globally or on a
/// per-client basis unless otherwise noted. A full list of regions is found in the
/// "Regions and Endpoints" document.
///
/// See http://docs.aws.amazon.com/general/latest/gr/rande.html for
/// information on AWS regions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region(Arc<String>);
impl AsRef<str> for Region {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Region {
    pub fn new(region: impl Into<String>) -> Self {
        Self(Arc::new(region.into()))
    }
}

/// The region to use when signing requests
///
/// Generally, user code will not need to interact with `SigningRegion`. See `[Region](crate::Region)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningRegion(Arc<String>);
impl AsRef<str> for SigningRegion {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Region> for SigningRegion {
    fn from(inp: Region) -> Self {
        SigningRegion(inp.0)
    }
}

/// The name of the service used to sign this request
///
/// Generally, user code should never interact with `SigningService` directly
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningService(Cow<'static, str>);
impl AsRef<str> for SigningService {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl SigningService {
    pub fn from_static(service: &'static str) -> Self {
        SigningService(Cow::Borrowed(service))
    }
}
