pub mod test_connection;

use aws_endpoint::AwsEndpointStage;
use aws_sig_auth::middleware::SigV4SigningStage;
use aws_sig_auth::signer::SigV4Signer;
use hyper::client::HttpConnector;
use hyper::Client as HyperClient;
use hyper_tls::HttpsConnector;
use smithy_http::body::SdkBody;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
use smithy_http_tower::dispatch::DispatchLayer;
use smithy_http_tower::map_request::MapRequestLayer;
use smithy_http_tower::parse_response::ParseResponseLayer;
use std::error::Error;
use tower::{Service, ServiceBuilder, ServiceExt};

type BoxError = Box<dyn Error + Send + Sync>;

pub type SdkError<E> = smithy_http::result::SdkError<E, hyper::Body>;
pub type SdkSuccess<T> = smithy_http::result::SdkSuccess<T, hyper::Body>;

pub struct Client<S> {
    inner: S,
}

impl<S> Client<S> {
    pub fn new(connector: S) -> Self {
        Client { inner: connector }
    }
}

impl Client<hyper::Client<HttpsConnector<HttpConnector>, SdkBody>> {
    pub fn default() -> Self {
        let https = HttpsConnector::new();
        let client = HyperClient::builder().build::<_, SdkBody>(https);
        Client { inner: client }
    }
}

impl<S> Client<S>
where
    S: Service<http::Request<SdkBody>, Response = http::Response<hyper::Body>>
        + Send
        + Clone
        + 'static,
    S::Error: Into<BoxError> + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    /// Dispatch this request to the network
    ///
    /// For ergonomics, this does not include the raw response for successful responses. To
    /// access the raw response use `call_raw`.
    pub async fn call<O, T, E, Retry>(&self, input: Operation<O, Retry>) -> Result<T, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<T, E>> + Send + Clone + 'static,
    {
        self.call_raw(input).await.map(|res| res.parsed)
    }

    pub async fn call_raw<O, R, E, Retry>(
        &self,
        input: Operation<O, Retry>,
    ) -> Result<SdkSuccess<R>, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<R, E>> + Send + Clone + 'static,
    {
        let signer = MapRequestLayer::for_mapper(SigV4SigningStage::new(SigV4Signer::new()));
        let endpoint_resolver = MapRequestLayer::for_mapper(AwsEndpointStage);
        let inner = self.inner.clone();
        let mut svc = ServiceBuilder::new()
            .layer(ParseResponseLayer::<O, Retry>::new())
            .layer(endpoint_resolver)
            .layer(signer)
            .layer(DispatchLayer::new())
            .service(inner);
        svc.ready_and().await?.call(input).await
    }
}

#[cfg(test)]
mod tests {
    use crate::Client;

    #[test]
    fn construct_default_client() {
        let _ = Client::default();
    }
}
