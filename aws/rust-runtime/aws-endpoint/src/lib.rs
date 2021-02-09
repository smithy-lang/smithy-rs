use aws_types::{Region, SigningRegion, SigningService};
use http::Uri;
use smithy_http::endpoint::{Endpoint, EndpointPrefix};
use smithy_http::middleware::MapRequest;
use smithy_http::operation::Request;
use smithy_http::property_bag::PropertyBag;
use std::error::Error;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

/// Endpoint to connect to an AWS Service
///
/// An `AwsEndpoint` captures all necessary information needed to connect to an AWS service, including:
/// - The URI of the endpoint (needed to actually send the request)
/// - The name of the service (needed downstream for signing)
/// - The signing region (which may differ from the actual region)
#[derive(Clone)]
pub struct AwsEndpoint {
    endpoint: Endpoint,
    signing_service: SigningService,
    signing_region: SigningRegion,
}

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Resolve the AWS Endpoint for a given region
///
/// Each individual service will generate their own implementation of `ResolveAwsEndpoint`. This implementation
/// may use endpoint discovery (if used by the service). The list of supported regions for a given service
/// will be codegenerated from `endpoints.json`.
pub trait ResolveAwsEndpoint: Send + Sync {
    // TODO: consider if we want modeled error variants here
    fn endpoint(&self, region: &Region) -> Result<AwsEndpoint, BoxError>;
}

/// Default AWS Endpoint Implementation
///
/// This is used as a temporary stub. Prior to GA, this will be replaced with specifically generated endpoint
/// resolvers for each service that model the endpoints for each service correctly. Some services differ
/// from the standard endpoint pattern.
pub struct DefaultAwsEndpointResolver {
    service: &'static str,
}

impl DefaultAwsEndpointResolver {
    pub fn for_service(service: &'static str) -> Self {
        Self { service }
    }
}

impl ResolveAwsEndpoint for DefaultAwsEndpointResolver {
    fn endpoint(&self, region: &Region) -> Result<AwsEndpoint, BoxError> {
        let uri = Uri::from_str(&format!(
            "https://{}.{}.amazonaws.com",
            region.as_ref(),
            self.service
        ))?;
        Ok(AwsEndpoint {
            endpoint: Endpoint::mutable(uri),
            signing_region: region.clone().into(),
            signing_service: SigningService::from_static(self.service),
        })
    }
}

type AwsEndpointProvider = Arc<dyn ResolveAwsEndpoint>;
fn get_endpoint_provider(config: &PropertyBag) -> Option<&AwsEndpointProvider> {
    config.get()
}

pub fn set_endpoint_provider(provider: AwsEndpointProvider, config: &mut PropertyBag) {
    config.insert(provider);
}

/// Middleware Stage to Add an Endpoint to a Request
///
/// AwsEndpointStage implements [`MapRequest`](smithy_http::middleware::MapRequest). It will:
/// 1. Load an endpoint provider from the property bag.
/// 2. Load an endpoint given the [`Region`](aws_types::Region) in the property bag.
/// 3. Apply the endpoint to the URI in the request
/// 4. Set the `SigningRegion` and `SigningService` in the property bag to drive downstream
/// signing middleware.
pub struct AwsEndpointStage;

#[derive(Debug)]
pub enum AwsEndpointStageError {
    NoEndpointProvider,
    NoRegion,
    EndpointResolutionError(BoxError),
}

impl Display for AwsEndpointStageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}
impl Error for AwsEndpointStageError {}

impl MapRequest for AwsEndpointStage {
    type Error = AwsEndpointStageError;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        request.augment(|mut http_req, config| {
            let provider =
                get_endpoint_provider(config).ok_or(AwsEndpointStageError::NoEndpointProvider)?;
            let region = config
                .get::<Region>()
                .ok_or(AwsEndpointStageError::NoRegion)?;
            let endpoint = provider
                .endpoint(region)
                .map_err(AwsEndpointStageError::EndpointResolutionError)?;
            config.insert(endpoint.signing_region);
            config.insert(endpoint.signing_service);
            endpoint
                .endpoint
                .set_endpoint(http_req.uri_mut(), config.get::<EndpointPrefix>());
            Ok(http_req)
        })
    }
}

#[cfg(test)]
mod test {
    use crate::{set_endpoint_provider, AwsEndpointStage, DefaultAwsEndpointResolver};
    use aws_types::{Region, SigningRegion, SigningService};
    use http::Uri;
    use smithy_http::body::SdkBody;
    use smithy_http::middleware::MapRequest;
    use smithy_http::operation;
    use std::sync::Arc;

    #[test]
    fn default_endpoint_updates_request() {
        let provider = Arc::new(DefaultAwsEndpointResolver::for_service("kinesis"));
        let req = http::Request::new(SdkBody::from(""));
        let region = Region::new("us-east-1");
        let mut req = operation::Request::new(req);
        {
            let mut conf = req.config_mut();
            conf.insert(region.clone());
            set_endpoint_provider(provider, &mut conf);
        };
        let req = AwsEndpointStage.apply(req).expect("should succeed");
        assert_eq!(
            req.config().get(),
            Some(&SigningRegion::from(region.clone()))
        );
        assert_eq!(
            req.config().get(),
            Some(&SigningService::from_static("kinesis"))
        );

        let (req, _conf) = req.into_parts();
        assert_eq!(
            req.uri(),
            &Uri::from_static("https://us-east-1.kinesis.amazonaws.com")
        );
    }
}
