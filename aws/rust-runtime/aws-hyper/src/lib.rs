use bytes::{Buf, Bytes};
use hyper::Client as HyperClient;
use operation::{middleware::OperationError, HttpRequestResponse, Operation, SdkBody};
use std::error::Error;
use tower::util::ReadyOneshot;
use tower::{Layer, Service};

type ResponseBody = hyper::Body;

#[derive(Debug)]
pub struct SdkResponse<O> {
    pub raw: http::Response<ResponseBody>,
    pub parsed: O,
}

pub enum SdkError<E> {
    DispatchFailure(Box<dyn Error>),
    ResponseError {
        raw: http::Response<ResponseBody>,
        err: Box<dyn Error>,
    },
    ServiceError {
        raw: http::Response<ResponseBody>,
        err: E,
    },
}

impl<E: Error + 'static> SdkError<E> {
    pub fn error(self) -> Box<dyn Error> {
        match self {
            SdkError::DispatchFailure(e) => e,
            SdkError::ResponseError { err, .. } => err,
            SdkError::ServiceError { err, .. } => Box::new(err),
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

impl<S> Client<S>
where
    S: Service<http::Request<SdkBody>, Response = http::Response<hyper::Body>> + Clone,
    S::Error: std::error::Error + 'static,
{
    pub async fn call<O, R, E>(
        &self,
        mut input: Operation<O>,
    ) -> Result<SdkResponse<R>, SdkError<E>>
    where
        // TODO: clean up the way that response handlers work—should they be disconnected from the operation,
        // at least at this level of the API?
        O: HttpRequestResponse<O = Result<R, E>>,
    {
        let ready_service = ReadyOneshot::new(self.inner.clone())
            .await
            .map_err(|e| SdkError::DispatchFailure(e.into()))?;

        let signer = OperationRequestMiddlewareLayer::for_middleware(SigningMiddleware::new());
        let endpoint_resolver = OperationRequestMiddlewareLayer::for_middleware(EndpointMiddleware);
        let mut ready_service =
            signer.layer(endpoint_resolver.layer(DispatchLayer.layer(ready_service)));
        let handler = input.response_handler.take().unwrap();
        let mut response: http::Response<hyper::Body> =
            ready_service.call(input).await.map_err(|e| match e {
                OperationError::DispatchError(e) => SdkError::DispatchFailure(Box::new(e)),
                OperationError::ConstructionError(e) => SdkError::DispatchFailure(e),
            })?;

        let parsed = handler.parse_unloaded(&mut response);
        let mut response = match parsed {
            Some(Ok(r)) => {
                return Ok(SdkResponse {
                    raw: response,
                    parsed: r,
                });
            }
            Some(Err(e)) => {
                return Err(SdkError::ServiceError {
                    raw: response,
                    err: e,
                });
            }
            None => response,
        };

        let body = match read_body(response.body_mut()).await {
            Ok(body) => body,
            Err(e) => {
                return Err(SdkError::ResponseError {
                    raw: response,
                    err: Box::new(e),
                });
            }
        };

        let response = response.map(|_| Bytes::from(body));
        let parsed = handler.parse_loaded(&response);
        let raw = response.map(hyper::Body::from);
        match parsed {
            Ok(parsed) => Ok(SdkResponse { raw, parsed }),
            Err(err) => Err(SdkError::ServiceError { raw, err }),
        }
    }
}

use http_body::Body;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use operation::endpoint::EndpointMiddleware;
use operation::middleware::{DispatchLayer, OperationRequestMiddlewareLayer};
use operation::signing_middleware::SigningMiddleware;

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
    use crate::{read_body, Client};
    use auth::{
        Credentials, HttpSignatureType, HttpSigningConfig, RequestConfig, ServiceConfig,
        SigningAlgorithm, SigningConfig,
    };
    use bytes::Bytes;
    use http::{Request, Response};
    use hyper::service::service_fn;
    use operation::endpoint::StaticEndpoint;
    use operation::{HttpRequestResponse, Operation, SdkBody};
    use pin_utils::core_reexport::fmt::Formatter;
    use std::error::Error;
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

    #[derive(Clone)]
    struct TestOperationParser;
    impl HttpRequestResponse for TestOperationParser {
        type O = Result<String, String>;

        fn parse_unloaded<B>(&self, _response: &mut Response<B>) -> Option<Self::O> {
            Some(Ok("Hello!".to_string()))
        }

        fn parse_loaded(&self, _response: &Response<Bytes>) -> Self::O {
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
        impl Error for TestError {};

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

        let test_request: Arc<Mutex<Option<http::Request<Bytes>>>> = Arc::new(Mutex::new(None));
        let http_service = service_fn(|request: Request<SdkBody>| async {
            let mut request = request;
            let body = read_body(request.body_mut()).await.unwrap();
            *(test_request.clone().lock().unwrap()) = Some(request.map(|_| Bytes::from(body)));
            Result::<Response<hyper::Body>, TestError>::Ok(Response::new(hyper::Body::from(
                "hello!",
            )))
        });
        //let x: () = http_service;
        //http_service.call(http::Request::new(SdkBody::from("123")));
        let client = Client {
            inner: http_service,
        };
        let response = client.call(operation).await;
        let request = test_request.lock().unwrap().take().unwrap();
        assert_eq!(
            request
                .headers()
                .keys()
                .map(|it| it.as_str())
                .collect::<Vec<_>>(),
            vec!["authorization", "x-amz-date"]
        );
        assert!(response.is_ok());
    }

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
    }
}
