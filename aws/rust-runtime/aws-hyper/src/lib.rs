use hyper::Client as HyperClient;
use smithy_http::body::SdkBody;
use smithy_http::operation;
use smithy_http::response::ParseHttpResponse;
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

impl<S> Client<S> {
    pub fn with_tracing(self) -> Client<RawRequestLogging<S>> {
        Client {
            inner: RawRequestLogging { inner: self.inner },
        }
    }
}

// In the future, this needs to use the CRT
#[derive(Clone)]
struct RetryStrategy {}

impl<Handler: Clone, R: Clone, Response, Error>
    tower::retry::Policy<operation::Operation<Handler, R>, Response, Error> for RetryStrategy
where
    R: RetryPolicy<Response, Error>,
{
    type Future = Pin<Box<dyn Future<Output = Self>>>;

    fn retry(
        &self,
        req: &Operation<Handler, R>,
        result: Result<&Response, &Error>,
    ) -> Option<Self::Future> {
        let _resp = req.retry_policy().should_retry(result)?;
        let next = self.clone();
        let fut = async move {
            tokio::time::sleep(Duration::new(5, 0)).await;
            next
        };
        Some(Box::pin(fut))
    }

    fn clone_request(&self, req: &Operation<Handler, R>) -> Option<Operation<Handler, R>> {
        req.try_clone()
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
    pub async fn call<O, R, E, Retry>(&self, input: Operation<O, Retry>) -> Result<R, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<R, E>> + Send + Clone + 'static,
        Retry: RetryPolicy<SdkSuccess<R>, SdkError<E>> + Send + Clone + 'static,
    {
        self.call_raw(input).await.map(|res| res.parsed)
    }

    pub async fn call_raw<O, R, E, Retry>(
        &self,
        input: Operation<O, Retry>,
    ) -> Result<SdkSuccess<R>, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<R, E>> + Send + Clone + 'static,
        Retry: RetryPolicy<SdkSuccess<R>, SdkError<E>> + Send + Clone + 'static,
    {
        let signer = MapRequestLayer::for_mapper(SigV4SigningStage::new(SigV4Signer::new()));
        let endpoint_resolver = MapRequestLayer::for_mapper(AwsEndpointStage);
        let inner = self.inner.clone();
        let mut svc = ServiceBuilder::new()
            .retry(RetryStrategy {})
            .map_request(|r: Operation<O, Retry>| r)
            .layer(ParseResponseLayer::<O>::new())
            .layer(endpoint_resolver)
            .layer(signer)
            .layer(DispatchLayer::new())
            .service(inner);
        svc.ready_and().await?.call(input).await

        //svc.call(input).await
        //todo!()
    }
}

use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use middleware_tracing::RawRequestLogging;
use operationwip::retry_policy::RetryPolicy;

use smithy_http::operation::Operation;
use std::future::Future;
use std::pin::Pin;

use aws_endpoint::AwsEndpointStage;
use aws_sig_auth::middleware::SigV4SigningStage;
use aws_sig_auth::signer::SigV4Signer;
use smithy_http_tower::dispatch::DispatchLayer;
use smithy_http_tower::map_request::MapRequestLayer;
use smithy_http_tower::parse_response::ParseResponseLayer;
use std::time::Duration;

#[cfg(test)]
mod test {
    use crate::BoxError;

    use bytes::Bytes;

    use http::{Request, Response};

    use pin_utils::core_reexport::task::{Context, Poll};
    use smithy_http::body::SdkBody;

    use smithy_http::response::ParseHttpResponse;

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::mpsc;

    #[derive(Clone)]
    struct TestService {
        f: fn(&http::Request<SdkBody>) -> http::Response<hyper::Body>,
        tx: mpsc::Sender<http::Request<SdkBody>>,
    }

    impl TestService {
        pub fn new(
            f: fn(&http::Request<SdkBody>) -> http::Response<hyper::Body>,
        ) -> (Self, mpsc::Receiver<http::Request<SdkBody>>) {
            let (tx, rx) = mpsc::channel();
            (TestService { f, tx }, rx)
        }
    }

    impl tower::Service<http::Request<SdkBody>> for TestService {
        type Response = http::Response<hyper::Body>;
        type Error = BoxError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request<SdkBody>) -> Self::Future {
            let response = (self.f)(&req);
            self.tx.send(req).unwrap();
            Box::pin(std::future::ready(Ok(response)))
        }
    }

    #[derive(Clone)]
    struct TestOperationParser;

    impl<B> ParseHttpResponse<B> for TestOperationParser
    where
        B: http_body::Body,
    {
        type Output = Result<String, String>;

        fn parse_unloaded(&self, _response: &mut Response<B>) -> Option<Self::Output> {
            Some(Ok("Hello!".to_string()))
        }

        fn parse_loaded(&self, _response: &Response<Bytes>) -> Self::Output {
            Ok("Hello!".to_string())
        }
    }

    /*
    #[tokio::test]
    async fn e2e_service() {
        #[derive(Debug)]
        struct TestError;

        impl std::fmt::Display for TestError {
            fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
                unimplemented!()
            }
        }
        impl Error for TestError {}

        let request = operation::Request::new(
            Request::builder()
                .uri("/some_url")
                .body(SdkBody::from("Hello"))
                .unwrap(),
        )
        .augment(|req, config| {
            config.insert(Region::new("some-region"));
            config.insert(UNIX_EPOCH + Duration::new(1611160427, 0));
            config.insert_signing_config(OperationSigningConfig::default_config("some-service"));
            use operationwip::signing_middleware::CredentialProviderExt;
            config.insert_credentials_provider(Arc::new(Credentials::from_keys(
                "access", "secret", None,
            )));
            config.insert_endpoint_provider(Arc::new(Endpoint::from_uri(Uri::from_static(
                "http://localhost:8000",
            ))));
            Result::<_, ()>::Ok(req)
        })
        .expect("valid request");

        let operation = Operation::new(request, TestOperationParser);

        let (svc, rx) = TestService::new(|_req| http::Response::new(hyper::Body::from("hello!")));
        let client = Client { inner: svc };
        let resp = client.call(operation).await;
        println!("{:?}", resp);
        let request = rx.try_recv().unwrap();

        assert_eq!(
            request
                .headers()
                .keys()
                .map(|it| it.as_str())
                .collect::<Vec<_>>(),
            vec!["host", "authorization", "x-amz-date"]
        );
        assert_eq!(request.headers().get(AUTHORIZATION).unwrap(), "AWS4-HMAC-SHA256 Credential=access/20210120/some-region/some-service/aws4_request, SignedHeaders=host, Signature=f179c6899f0a11051a11dc1bb022252b0741953663bc5ff33dfa2abfed51e0b1");
    }*/
}
