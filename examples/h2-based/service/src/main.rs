use server_sdk::server::protocol::rest_json_1::rejection::RequestRejection;
use server_sdk::{
    SampleService, SampleServiceConfig, error::SampleOperationError, input::SampleOperationInput,
    output::SampleOperationOutput, server::plugin::IdentityPlugin,
};

use bytes::Bytes;
use h2::server::{self, SendResponse};
use h2::{RecvStream, SendStream};
use http::StatusCode;
use http::{Method, Request, Response, Version};
//use hyper::Body;
use http_body::Body;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::{TcpListener, TcpStream};
use tower::{ServiceExt, layer::util::Identity};

async fn my_handler(
    _input: SampleOperationInput,
) -> Result<SampleOperationOutput, SampleOperationError> {
    Ok(SampleOperationOutput {
        result: "fahad".to_owned(),
    })
}

// Convert HTTP response to H2 response
// fn convert_http_to_h2_response(response: Response<hyper::Body>) -> (Response<()>, hyper::Body) {
//     let (parts, body) = response.into_parts();
//     let h2_response = Response::from_parts(parts, ());
//     (h2_response, body)
// }

// async fn handle_connection<S>(
//     stream: TcpStream,
//     service: S,
// ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
// where
//     S: tower::Service<Request<Body>, Response = Response<hyper::Body>> + Clone + Send + 'static,
//     S::Error: std::error::Error + Send + Sync + 'static,
//     S::Future: Send + 'static,
// {
//     let mut connection = server::handshake(stream).await?;

//     loop {
//         match connection.accept().await {
//             Ok(Some((request, respond))) => {
//                 let mut service = service.clone();

//                 tokio::spawn(async move {
//                     // Read the request body
//                     let mut body_bytes = Vec::new();
//                     let mut recv_stream = request.into_body();

//                     while let Some(chunk) = recv_stream.data().await {
//                         match chunk {
//                             Ok(data) => {
//                                 body_bytes.extend_from_slice(&*data);
//                                 // Release the flow control
//                                 let _ = recv_stream.flow_control().release_capacity(data.len());
//                             }
//                             Err(e) => {
//                                 eprintln!("Error reading request body: {}", e);
//                                 return;
//                             }
//                         }
//                     }

//                     // Convert to HTTP request
//                     let http_request = Request::builder()
//                         .method(Method::POST)
//                         .uri("/sample")
//                         .version(Version::HTTP_2)
//                         .header("content-type", "application/json")
//                         .body(Body::from(body_bytes))
//                         .unwrap();

//                     // Call the Tower service
//                     match service.ready().await {
//                         Ok(ready_service) => {
//                             match ready_service.call(http_request).await {
//                                 Ok(response) => {
//                                     let (h2_response, body) = convert_http_to_h2_response(response);

//                                     // Send response headers
//                                     let mut send_stream =
//                                         match respond.send_response(h2_response, false) {
//                                             Ok(stream) => stream,
//                                             Err(e) => {
//                                                 eprintln!("Error sending response headers: {}", e);
//                                                 return;
//                                             }
//                                         };

//                                     // Send response body
//                                     match hyper::body::to_bytes(body).await {
//                                         Ok(body_bytes) => {
//                                             if let Err(e) = send_stream.send_data(body_bytes, true)
//                                             {
//                                                 eprintln!("Error sending response body: {}", e);
//                                             }
//                                         }
//                                         Err(e) => {
//                                             eprintln!("Error converting response body: {}", e);
//                                         }
//                                     }
//                                 }
//                                 Err(e) => {
//                                     eprintln!("Service error: {}", e);
//                                     // Send 500 Internal Server Error
//                                     let error_response =
//                                         Response::builder().status(500).body(()).unwrap();
//                                     let _ = respond.send_response(error_response, true);
//                                 }
//                             }
//                         }
//                         Err(e) => {
//                             eprintln!("Service not ready: {}", e);
//                             let error_response = Response::builder().status(503).body(()).unwrap();
//                             let _ = respond.send_response(error_response, true);
//                         }
//                     }
//                 });
//             }
//             Ok(None) => {
//                 // Connection closed
//                 break;
//             }
//             Err(e) => {
//                 eprintln!("Error accepting request: {}", e);
//                 break;
//             }
//         }
//     }

//     Ok(())
// }

async fn handle_connection<S>(
    stream: TcpStream,
    service: S,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tower::Service<
            Request<LoggingBody>,
            Response = Response<aws_smithy_http_server::body::BoxBody>,
        > + Clone
        + Send
        + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    let mut connection = server::handshake(stream).await?;

    while let Some(result) = connection.accept().await {
        let (request, mut respond) = result?;
        println!("Received request: {:?}", request);

        let mut s = service.clone();

        tokio::spawn(async move {
            s.ready()
                .await
                .expect("Service should be ready")
                .call(request.map(LoggingBody::new))
                .await
                .map_err(|e| {
                    eprintln!("Service error: {}", e);
                    RequestRejection::NotAcceptable
                })
                .and_then(|response| {
                    // let mut send = respond.send_response(response, false)?;
                    // send.send_data(Bytes::from("Response body"), true)?;
                    let response = Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/plain")
                        .body(())
                        .unwrap();

                    // Send the response headers
                    let mut send = respond.send_response(response, false).unwrap();

                    // Send the body
                    send.send_data(Bytes::from("helloworld"), true).unwrap();

                    Ok(())
                })
                .unwrap_or_else(|e| {
                    eprintln!("Error handling request: {}", e);
                    let error_response = Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(())
                        .unwrap();
                    let _ = respond.send_response(error_response, true);
                });

            // if let Err(e) = handle_request(request, respond).await {
            //     eprintln!("Request handling error: {}", e);
            // }
        });
    }

    Ok(())
}

struct LoggingBody {
    inner: RecvStream,
    collected_data: Vec<u8>,
    finished: bool,
}

impl LoggingBody {
    fn new(recv_stream: RecvStream) -> Self {
        Self {
            inner: recv_stream,
            collected_data: Vec::new(),
            finished: false,
        }
    }
}

impl Body for LoggingBody {
    type Data = Bytes;
    //type Error = h2::Error;
    type Error = RequestRejection;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }

        let poll_result = self.inner.poll_data(cx);

        match poll_result {
            Poll::Ready(Some(Ok(chunk))) => {
                // Store the chunk data for logging
                self.collected_data.extend_from_slice(&chunk);

                // Release flow control capacity
                let _ = self.inner.flow_control().release_capacity(chunk.len());

                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                // Log the complete body when finished
                let body_string = String::from_utf8_lossy(&self.collected_data);
                println!("Request body: {}", body_string);
                println!("Request body length: {} bytes", self.collected_data.len());
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(RequestRejection::NotAcceptable))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        self.inner
            .poll_trailers(cx)
            .map_err(|_e| RequestRejection::NotAcceptable)
    }
}

// Function that accepts any Body trait and prints information
async fn print_body_info<B>(mut body: B) -> Result<(), B::Error>
where
    B: Body + Unpin,
    B: Body<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let mut total_size = 0;
    let mut chunk_count = 0;

    println!("--- Body Information ---");

    while let Some(chunk_result) = body.data().await {
        match chunk_result {
            Ok(chunk) => {
                chunk_count += 1;
                total_size += chunk.len();
                println!("Chunk {}: {} bytes", chunk_count, chunk.len());
                // You could also print chunk content here if needed
                // println!("Chunk content: {}", String::from_utf8_lossy(&chunk));
            }
            Err(e) => {
                println!("Error reading chunk: {}", e);
                return Err(e);
            }
        }
    }

    println!("Total chunks: {}", chunk_count);
    println!("Total size: {} bytes", total_size);
    println!("--- End Body Information ---");

    Ok(())
}

async fn handle_request(
    mut request: Request<RecvStream>,
    mut respond: SendResponse<Bytes>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Collect the complete request body
    // let mut body = request.into_body();
    // let mut body_data = Vec::new();

    // while let Some(chunk) = body.data().await {
    //     let chunk = chunk?;
    //     body_data.extend_from_slice(&chunk);
    //     let _ = body.flow_control().release_capacity(chunk.len());
    // }
    // Convert body to string for logging
    // let body_string = String::from_utf8_lossy(&body_data);
    // println!("Request body: {}", body_string);
    // println!("Request body length: {} bytes", body_data.len());

    let logging_body = LoggingBody::new(request.into_body());

    // Use our generic function to print body info
    print_body_info(logging_body).await?;

    // Create the response
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain")
        .body(())
        .unwrap();

    // Send the response headers
    let mut send = respond.send_response(response, false)?;

    // Send the body
    send.send_data(Bytes::from("helloworld"), true)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Starting H2 server on 0.0.0.0:9090");

    // Build the Tower service (same as before)
    let config = SampleServiceConfig::builder().build();

    let app =
        SampleService::builder::<LoggingBody, Identity, IdentityPlugin, IdentityPlugin>(config)
            .sample_operation(my_handler)
            .build()
            .expect("failed to build");

    //let make_app = app.into_make_service();
    // let bind = "localhost:9090"
    //     .to_string()
    //     .parse()
    //     .expect("unable to parse the server bind address and port");
    // let server = hyper::Server::bind(&bind).serve(make_app);
    // if let Err(err) = server.await {
    //     eprintln!("server error: {}", err);
    // }

    // Create TCP listener
    let listener = TcpListener::bind("0.0.0.0:9090").await?;
    println!("H2 server listening on 0.0.0.0:9090");

    // Accept connections and handle them with H2
    while let Ok((stream, addr)) = listener.accept().await {
        println!("New connection from: {}", addr);
        let service = app.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, service).await {
                eprintln!("Connection error from {}: {}", addr, e);
            }
            // if let Err(e) = handle_connection(stream, service).await {
            //     eprintln!("Connection error from {}: {}", addr, e);
            // }
        });
    }

    Ok(())
}
