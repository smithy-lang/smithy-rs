use bytes::{Buf, Bytes};
use hyper::Client as HyperClient;
use operation::{middleware::OperationError, Operation, ParseHttpResponse, SdkBody};
use std::error::Error;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};

type SdkSuccess<O> = _SdkSuccess<O, hyper::Body>;
type SdkError<E> = _SdkError<E, hyper::Body>;
type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug)]
pub struct _SdkSuccess<O, B> {
    pub raw: http::Response<B>,
    pub parsed: O,
}

#[derive(Debug)]
pub enum _SdkError<E, B> {
    ConstructionFailure(Box<dyn Error>),
    DispatchFailure(Box<dyn Error>),
    ResponseError {
        raw: http::Response<B>,
        err: Box<dyn Error>,
    },
    ServiceError {
        raw: http::Response<B>,
        err: E,
    },
}

pub fn sdk_result<T, E, B>(
    parsed: Result<T, E>,
    raw: http::Response<B>,
) -> Result<_SdkSuccess<T, B>, _SdkError<E, B>> {
    match parsed {
        Ok(parsed) => Ok(_SdkSuccess { raw, parsed }),
        Err(err) => Err(_SdkError::ServiceError { raw, err }),
    }
}

impl<E: Error + 'static> SdkError<E> {
    pub fn error(self) -> Box<dyn Error> {
        match self {
            SdkError::DispatchFailure(e) => e,
            SdkError::ResponseError { err, .. } => err,
            SdkError::ServiceError { err, .. } => Box::new(err),
            SdkError::ConstructionFailure(e) => e,
        }
    }
}

pub struct Client<S> {
    inner: S,
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

fn operation_error<OE, E, B>(o: OperationError<OE>) -> _SdkError<E, B>
where
    OE: Into<BoxError>,
{
    match o {
        OperationError::DispatchError(e) => _SdkError::DispatchFailure(e.into()),
        OperationError::ConstructionError(e) => _SdkError::ConstructionFailure(e),
    }
}

async fn load_response<B, T, E, O>(
    mut response: http::Response<B>,
    handler: &O,
) -> Result<_SdkSuccess<T, B>, _SdkError<E, B>>
where
    B: http_body::Body + Unpin,
    B: From<Bytes>,
    B::Error: Error + 'static,
    O: ParseHttpResponse<B, Output = Result<T, E>>,
{
    if let Some(parsed_response) = handler.parse_unloaded(&mut response) {
        return sdk_result(parsed_response, response);
    }

    let body = match read_body(response.body_mut()).await {
        Ok(body) => body,
        Err(e) => {
            return Err(_SdkError::ResponseError {
                raw: response,
                err: Box::new(e),
            });
        }
    };

    let response = response.map(|_| Bytes::from(body));
    let parsed = handler.parse_loaded(&response);
    return sdk_result(parsed, response.map(B::from));
}

pub struct LoadResponseMiddleware<S> {
    inner: S,
}

pub struct ParseResponseLayer;

impl<S> Layer<S> for ParseResponseLayer
where
    S: Service<operation::Request>,
{
    type Service = LoadResponseMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoadResponseMiddleware { inner }
    }
}

impl<S, O, T, E, B, R, OE> tower::Service<operation::Operation<O, R>> for LoadResponseMiddleware<S>
where
    S: Service<operation::Request, Response = http::Response<B>, Error = OperationError<OE>>,
    OE: Into<BoxError>,
    S::Future: 'static,
    O: ParseHttpResponse<B, Output = Result<T, E>> + 'static,
    B: http_body::Body + Unpin,
    B: From<Bytes>,
    B::Error: Error + 'static,
{
    type Response = _SdkSuccess<T, B>;
    type Error = _SdkError<E, B>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(operation_error)
    }

    fn call(&mut self, req: Operation<O, R>) -> Self::Future {
        let resp = self.inner.call(req.request);
        let handler = req.response_handler;
        let fut = async move {
            match resp.await {
                // TODO: split variant so they we don't retry missing credentials
                Err(e) => Err(operation_error::<OE, E, B>(e)),
                Ok(resp) => load_response(resp, &handler).await,
            }
        };
        Box::pin(fut)
    }
}

impl<S> Client<S>
where
    S: Service<http::Request<SdkBody>, Response = http::Response<hyper::Body>> + Clone,
    S::Error: Into<BoxError> + 'static,
    S::Future: 'static,
{
    pub async fn call<O, R, E, Retry>(
        &self,
        input: Operation<O, Retry>,
    ) -> Result<SdkSuccess<R>, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<R, E>> + 'static,
        Retry: RetryPolicy<Result<R, E>> + 'static,
    {
        let signer = OperationRequestMiddlewareLayer::for_middleware(SigningMiddleware::new());
        let endpoint_resolver = OperationRequestMiddlewareLayer::for_middleware(EndpointMiddleware);
        let mut inner = self.inner.clone();
        // TODO: reorder to call ready_and on the entire stack
        let inner = inner
            .ready_and()
            .await
            .map_err(|e| _SdkError::DispatchFailure(e.into()))?;
        let mut svc = ServiceBuilder::new()
            .layer(ParseResponseLayer)
            .layer(endpoint_resolver)
            .layer(signer)
            .layer(DispatchLayer)
            .service(inner);
        svc.call(input).await
    }
}

use http_body::Body;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use middleware_tracing::RawRequestLogging;
use operation::endpoint::EndpointMiddleware;
use operation::middleware::{DispatchLayer, OperationRequestMiddlewareLayer};
use operation::retry_policy::RetryPolicy;
use operation::signing_middleware::SigningMiddleware;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

async fn read_body<B: http_body::Body>(body: B) -> Result<Vec<u8>, B::Error> {
    let mut output = Vec::new();
    pin_utils::pin_mut!(body);
    while let Some(buf) = body.data().await {
        let mut buf = buf?;
        while buf.has_remaining() {
            output.extend_from_slice(buf.chunk());
            buf.advance(buf.chunk().len())
        }
    }
    Ok(output)
}

#[cfg(test)]
mod test {
    use crate::{BoxError, Client};
    use auth::Credentials;
    use bytes::Bytes;
    use http::header::AUTHORIZATION;
    use http::{Request, Response, Uri};
    use operation::endpoint::StaticEndpoint;
    use operation::region::Region;
    use operation::signing_middleware::SigningConfigExt;
    use operation::{Operation, ParseHttpResponse, SdkBody};
    use pin_utils::core_reexport::task::{Context, Poll};
    use pin_utils::core_reexport::time::Duration;
    use std::error::Error;
    use std::fmt::Formatter;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{mpsc, Arc};
    use std::time::UNIX_EPOCH;

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
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

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
        );

        request
            .config
            .lock()
            .unwrap()
            .insert(Region::new("some-region"));
        request
            .config
            .lock()
            .unwrap()
            .insert(UNIX_EPOCH + Duration::new(1611160427, 0));

        request
            .config
            .lock()
            .unwrap()
            .insert_signing_config(auth::OperationSigningConfig::default_config("some-service"));
        use operation::signing_middleware::CredentialProviderExt;
        request
            .config
            .lock()
            .unwrap()
            .insert_credentials_provider(Arc::new(Credentials::from_static("access", "secret")));

        use operation::endpoint::EndpointProviderExt;
        request
            .config
            .lock()
            .unwrap()
            .insert_endpoint_provider(Arc::new(StaticEndpoint::from_uri(Uri::from_static(
                "http://localhost:8000",
            ))));
        let operation = Operation {
            request,
            response_handler: TestOperationParser,
            retry_policy: (),
        };

        let (svc, rx) = TestService::new(|_req| http::Response::new(hyper::Body::from("hello!")));
        let client = Client { inner: svc };
        //handle.sen
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
    }

    /*
    #[test]
    fn real_service() {
        let operation = Operation {
            base: Request::builder()
                .uri("/some_url")
                .body(SdkBody::from("Hello"))
                .unwrap(),
            signing_config: SigningConfig::Http(HttpSigningConfig {
                algorithm: SigningAlgorithm::SigV4,
                signature_type: HttpSignatureType::HttpRequestHeaders,
                service_config: ServiceConfig {
                    service: "svc".to_string(),
                    region: "region".to_string(),
                },
                request_config: RequestConfig {
                    request_ts: || SystemTime::now(),
                },
                double_uri_encode: false,
                normalize_uri_path: true,
                omit_session_token: false,
            }),
            credentials_provider: Box::new(Credentials::from_static("key", "secret", None)),
            endpoint_config: Box::new(StaticEndpoint::from_service_region("dynamodb", "us-east-1")),
            response_handler: Some(Box::new(TestOperationParser)),
        };
        let client = Client::default();
        let _ = client.call(operation);
    }*/
}
