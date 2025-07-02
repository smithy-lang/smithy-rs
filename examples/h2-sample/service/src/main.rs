use aws_smithy_http_server::{protocol::rest_json_1::RestJson1, response::IntoResponse};
use server_sdk::server::protocol::rest_json_1::rejection::RequestRejection;
use server_sdk::{
    SampleService, SampleServiceConfig, error::SampleOperationError, input::SampleOperationInput,
    operation_shape::SampleOperation, output::SampleOperationOutput,
    protocol_serde::shape_sample_operation, server::plugin::IdentityPlugin,
};

use bytes::Bytes;
use h2::RecvStream;
use h2::server::{self};
use http::{Request, Response};
use http_body::Body;
use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::net::{TcpListener, TcpStream};
use tower::{ServiceExt, layer::util::Identity};

async fn sample_handler(
    _input: SampleOperationInput,
) -> Result<SampleOperationOutput, SampleOperationError> {
    Ok(SampleOperationOutput {
        result: "some output from the handler".to_owned(),
    })
}

#[derive(Clone)]
struct CachingService<B> {
    cache:
        Arc<Mutex<HashMap<SampleOperationInput, Response<aws_smithy_http_server::body::BoxBody>>>>,
    _phantom: std::marker::PhantomData<B>,
}

impl<B> CachingService<B> {
    fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            _phantom: std::marker::PhantomData,
        }
    }

    fn generate_cache_key(&self, method: &http::Method, uri: &http::Uri, body: &[u8]) -> String {
        format!(
            "{}:{}:{}",
            method,
            uri,
            std::str::from_utf8(body).unwrap_or("")
        )
    }

    async fn handle_request(
        &self,
        req: Request<B>,
    ) -> Result<Response<aws_smithy_http_server::body::BoxBody>, Infallible>
    where
        B: aws_smithy_http_server::body::HttpBody + Send + 'static,
        B::Data: Send,
        aws_smithy_http_server::protocol::rest_json_1::rejection::RequestRejection: From<B::Error>,
    {
        let input = match shape_sample_operation::de_sample_operation_http_request(req).await {
            Ok(input) => input,
            Err(e) => {
                let error_response = ::aws_smithy_http_server::response::IntoResponse::<::aws_smithy_http_server::protocol::rest_json_1::RestJson1>::into_response(::aws_smithy_http_server::protocol::rest_json_1::runtime_error::RuntimeError::from(e));
                return Ok(error_response);
            }
        };

        // {
        //     let cache_guard = self.cache.lock().unwrap();
        //     if let Some(cached_response) = cache_guard.get(&input) {
        //         return Ok(cached_response.clone());
        //     }
        // }

        let output = SampleOperationOutput {
            result: format!("Hello from cache for input: {}", input.input_value),
        };

        // {
        //     let mut cache_guard = self.cache.lock().unwrap();
        //     cache_guard.insert(cache_key, response.clone());
        // }

        let ok_response = ::aws_smithy_http_server::response::IntoResponse::<
            ::aws_smithy_http_server::protocol::rest_json_1::RestJson1,
        >::into_response(output);

        Ok(ok_response)
    }
}

impl<B> tower::Service<Request<B>> for CachingService<B>
where
    B: aws_smithy_http_server::body::HttpBody + Send + 'static,
    B::Data: Send,
    aws_smithy_http_server::protocol::rest_json_1::rejection::RequestRejection: From<B::Error>,
{
    type Response = Response<aws_smithy_http_server::body::BoxBody>;
    type Error = Infallible;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let cache = self.cache.clone();

        Box::pin(async move {
            let service = CachingService {
                cache,
                _phantom: std::marker::PhantomData,
            };
            service.handle_request(req).await
        })
    }
}

async fn handle_connection<S>(
    stream: TcpStream,
    service: S,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tower::Service<
            Request<H2BasedBody>,
            Response = Response<aws_smithy_http_server::body::BoxBody>,
            Error = Infallible,
        > + Clone
        + Send
        + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    let mut connection = server::handshake(stream).await?;

    while let Some(result) = connection.accept().await {
        let (request, mut response_sender) = result?;

        let mut handler_service = service.clone();

        tokio::spawn(async move {
            let s = match handler_service.ready().await {
                Ok(service) => service,
                Err(e) => {
                    eprintln!("Service not ready: {}", e);
                    return;
                }
            };

            let handler_response = match s.call(request.map(H2BasedBody::new)).await {
                Ok(response) => response,
                Err(e) => {
                    eprintln!("Service call failed: {}", e);
                    return;
                }
            };

            // Split the response into parts and body
            let (parts, mut body) = handler_response.into_parts();

            // Create H2 response with just the headers (empty body)
            let h2_response = Response::from_parts(parts, ());

            // Send the response headers
            let mut send_stream = match response_sender.send_response(h2_response, false) {
                Ok(sender) => sender,
                Err(e) => {
                    eprintln!("Failed to send response headers: {}", e);
                    return;
                }
            };

            // Stream body data chunk by chunk
            while let Some(chunk) = body.data().await {
                match chunk {
                    Ok(data) => {
                        if let Err(e) = send_stream.send_data(data, false) {
                            eprintln!("Error sending chunk: {}", e);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            // Send end of stream
            if let Err(e) = send_stream.send_data(Bytes::new(), true) {
                eprintln!("Failed to send end of stream: {}", e);
            }
        });
    }

    Ok(())
}

struct H2BasedBody {
    h2_stream: RecvStream,
    finished: bool,
}

impl H2BasedBody {
    fn new(recv_stream: RecvStream) -> Self {
        Self {
            h2_stream: recv_stream,
            finished: false,
        }
    }
}

impl Body for H2BasedBody {
    type Data = Bytes;
    type Error = RequestRejection;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }

        let poll_result = self.h2_stream.poll_data(cx);

        match poll_result {
            Poll::Ready(Some(Ok(chunk))) => {
                let _ = self.h2_stream.flow_control().release_capacity(chunk.len());

                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(_e))) => Poll::Ready(Some(Err(RequestRejection::NotAcceptable))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        self.h2_stream
            .poll_trailers(cx)
            .map_err(|_e| RequestRejection::NotAcceptable)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = SampleServiceConfig::builder().build();

    let caching_service = CachingService::<H2BasedBody>::new();
    let app =
        SampleService::builder::<H2BasedBody, Identity, IdentityPlugin, IdentityPlugin>(config)
            .sample_operation_custom(caching_service)
            .build()
            .map_err(|e| {
                eprintln!("Failed to build service: {}", e);
                e
            })?;

    let listener = TcpListener::bind("0.0.0.0:9090").await?;
    println!("H2 server listening on 0.0.0.0:9090");
    println!(
        r#"You can use the following command to test it:
    
    curl --http2-prior-knowledge -v localhost:9090/sample -X POST -H 'Content-type:application/json' -d '{{"inputValue":"any input you want"}}'"#
    );

    // Accept connections and handle them with H2
    while let Ok((stream, addr)) = listener.accept().await {
        let service = app.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, service).await {
                eprintln!("Connection error from {}: {}", addr, e);
            }
        });
    }

    Ok(())
}
