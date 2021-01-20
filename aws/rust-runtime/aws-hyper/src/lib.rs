use bytes::{Buf, Bytes};
use hyper::Client as HyperClient;
use operation::{middleware::OperationError, Operation, ParseHttpResponse, SdkBody};
use std::error::Error;
use tower::util::ReadyOneshot;
use tower::{Service, ServiceBuilder};

type ResponseBody = hyper::Body;

#[derive(Debug)]
pub struct SdkResponse<O> {
    pub raw: http::Response<ResponseBody>,
    pub parsed: O,
}

#[derive(Debug)]
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

impl<S> Client<S> {
    pub fn with_tracing(self) -> Client<RawRequestLogging<S>> {
        Client {
            inner: RawRequestLogging { inner: self.inner },
        }
    }
}

impl<S> Client<S>
where
    S: Service<http::Request<SdkBody>, Response = http::Response<hyper::Body>> + Clone,
    S::Error: std::error::Error + 'static,
{
    pub async fn call<O, R, E>(&self, input: Operation<O>) -> Result<SdkResponse<R>, SdkError<E>>
    where
        O: ParseHttpResponse<hyper::Body, Output = Result<R, E>>,
    {
        let ready_service = ReadyOneshot::new(self.inner.clone())
            .await
            .map_err(|e| SdkError::DispatchFailure(e.into()))?;

        let signer = OperationRequestMiddlewareLayer::for_middleware(SigningMiddleware::new());
        let endpoint_resolver = OperationRequestMiddlewareLayer::for_middleware(EndpointMiddleware);
        let mut ready_service = ServiceBuilder::new()
            .layer(endpoint_resolver)
            .layer(signer)
            .layer(DispatchLayer)
            .service(ready_service);
        // TODO: enable operations to specify their own extra middleware to add
        let handler = input.response_handler;
        let mut response: http::Response<hyper::Body> = ready_service
            .call(input.request)
            .await
            .map_err(|e| match e {
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
use middleware_tracing::RawRequestLogging;
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
    use auth::Credentials;
    use bytes::Bytes;
    use http::header::AUTHORIZATION;
    use http::{Request, Response, Uri};
    use hyper::service::service_fn;
    use operation::endpoint::StaticEndpoint;
    use operation::signing_middleware::SigningConfigExt;
    use operation::{Operation, ParseHttpResponse, SdkBody};
    use pin_utils::core_reexport::fmt::Formatter;
    use pin_utils::core_reexport::time::Duration;
    use std::error::Error;
    use std::sync::{Arc, Mutex};
    use std::time::UNIX_EPOCH;

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
        impl Error for TestError {};

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
            .insert_signing_config(auth::SigningConfig::default_config(
                auth::ServiceConfig {
                    service: "some-service".into(),
                    region: "some-region".into(),
                },
                auth::RequestConfig {
                    // 1/20/2021
                    request_ts: || UNIX_EPOCH + Duration::new(1611160427, 0),
                },
            ));
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
            response_handler: Box::new(TestOperationParser),
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
        let client = Client {
            inner: http_service,
        };
        let _ = client
            .call(operation)
            .await
            .expect("operation should succeed");
        let request = test_request.lock().unwrap().take().unwrap();
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
