# Writing and debugging a low-level feature that relies on HTTP

## Background

This article came about as a result of all the difficulties I encountered while developing the request checksums feature
laid out in the internal-only Flexible Checksums spec _(the feature is also highlighted in [this public blog post][S3 Flexible Checksums].)_
I spent much more time developing the feature than I had anticipated. In this article, I'll talk about:

- How the SDK sends requests with a body
- How the SDK sends requests with a streaming body
- The various issues I encountered and how I addressed them
- Key takeaways for contributors developing similar low-level features

## How the SDK sends requests with a body

All interactions between the SDK and a service are modeled as ["operations"][HTTP-based operations]. Operations contain:

- A base HTTP request (with a potentially streaming body)
- A typed property bag of configuration options
- A fully generic response handler

Users create operations piecemeal with a fluent builder. The options set in the builder are then used to create the
inner HTTP request, becoming headers or triggering specific request-building functionality (In this case, calculating a
checksum and attaching it either as a header or a trailer.)

Here's [an example from the QLDB SDK of creating a body] from inputs and inserting it into the request to be sent:

```rust,ignore
let body = aws_smithy_http::body::SdkBody::from(
    crate::operation_ser::serialize_operation_crate_operation_send_command(&self)?,
);

if let Some(content_length) = body.content_length() {
    request = aws_smithy_http::header::set_request_header_if_absent(
        request,
        http::header::CONTENT_LENGTH,
        content_length,
    );
}
let request = request.body(body).expect("should be valid request");
```

Most all request body creation in the SDKs looks like that. Note how it automatically sets the `Content-Length` header
whenever the size of the body is known; It'll be relevant later. The body is read into memory and can be inspected
before the request is sent. This allows for things like calculating a checksum and then inserting it into the request
as a header.

## How the SDK sends requests with a streaming body

Often, sending a request with a streaming body looks much the same. However, it's not possible to read a streaming
body until you've sent the request. Any metadata that needs to be calculated by inspecting the body must be sent as
trailers. Additionally, some metadata, like `Content-Length`, can't be sent as a trailer at all.
[MDN maintains a helpful list] of metadata that can only be sent as a header.

```rust,ignore
// When trailers are set, we must send an AWS-specific header that lists them named `x-amz-trailer`.
// For example, when sending a SHA256 checksum as a trailer,
// we have to send an `x-amz-trailer` header telling the service to watch out for it:
request
    .headers_mut()
    .insert(
        http::header::HeaderName::from_static("x-amz-trailer"),
        http::header::HeaderValue::from_static("x-amz-checksum-sha256"),
    );
```

## The issues I encountered while implementing checksums for streaming request bodies

### `Content-Encoding: aws-chunked`

When sending a request body with trailers, we must use an AWS-specific content encoding called `aws-chunked`. To encode
a request body for `aws-chunked` requires us to know the length of each chunk we're going to send before we send it. We
have to prefix each chunk with its size in bytes, represented by one or more [hexadecimal] digits. To close the body, we
send a final chunk with a zero. For example, the body "Hello world" would look like this when encoded:

```text
B\r\n
Hello world\r\n
0\r\n
```

When sending a request body encoded in this way, we need to set two length headers:

- `Content-Length` is the length of the entire request body, including the chunk size prefix and zero terminator. In the
  example above, this would be 19.
- `x-amz-decoded-content-length` is the length of the decoded request body. In the example above, this would be 11.

_**NOTE:** [`Content-Encoding`][Content-Encoding] is distinct from [`Transfer-Encoding`][Transfer-Encoding]. It's possible to
construct a request with both `Content-Encoding: chunked` AND `Transfer-Encoding: chunked`, although we don't ever need
to do that for SDK requests._

### S3 requires a `Content-Length` unless you also set `Transfer-Encoding: chunked`

S3 does not require you to send a `Content-Length` header if you set the `Transfer-Encoding: chunked` header. That's
very helpful because it's not always possible to know the total length of a stream of bytes if that's what you're
constructing your request body from. However, when sending trailers, this part of the spec can be misleading.

1. When sending a streaming request, we must send metadata like checksums as trailers
2. To send a request body with trailers, we must set the `Content-Encoding: aws-chunked` header
3. When using `aws-chunked` encoding for a request body, we must set the `x-amz-decoded-content-length` header with the
   pre-encoding length of the request body.

This means that we can't actually avoid having to know and specify the length of the request body when sending a request
to S3. This turns out to not be much of a problem for common use of the SDKs because most streaming request bodies are
constructed from files. In these cases we can ask the operating system for the file size before sending the request. So
long as that size doesn't change during sending of the request, all is well. In any other case, the request will fail.

### Adding trailers to a request changes the size of that request

Headers don't count towards the size of a request body, but trailers do. That means we need to take trailers (which
aren't sent until after the body) into account when setting the `Content-Length` header (which are sent before the
body.) This means that without setting `Transfer-Encoding: chunked`, the SDKs only support trailers of known length.
In the case of checksums, we're lucky because they're always going to be the same size. We must also take into account
the fact that checksum values are base64 encoded before being set (this lengthens them.)

### `hyper` supports HTTP request trailers but isn't compatible with `Content-Encoding: aws-chunked`

This was a big source of confusion for me, and I only figured out what was happening with the help of [@seanmonstar].
When using `aws-chunked` encoding, the trailers have to be appended to the body as part of `poll_data` instead of
relying on the `poll_trailers` method. The working `http_body::Body` implementation of an `aws-chunked` encoded body
looked like this:

```rust,ignore
impl Body for AwsChunkedBody<Inner> {
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        if *this.already_wrote_trailers {
            return Poll::Ready(None);
        }

        if *this.already_wrote_chunk_terminator {
            return match this.inner.poll_trailers(cx) {
                Poll::Ready(Ok(trailers)) => {
                    *this.already_wrote_trailers = true;
                    let total_length_of_trailers_in_bytes = this.options.trailer_lens.iter().sum();

                    Poll::Ready(Some(Ok(trailers_as_aws_chunked_bytes(
                        total_length_of_trailers_in_bytes,
                        trailers,
                    ))))
                }
                Poll::Pending => Poll::Pending,
                Poll::Ready(err) => Poll::Ready(Some(err)),
            };
        };

        match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(mut data))) => {
                let bytes = if *this.already_wrote_chunk_size_prefix {
                    data.copy_to_bytes(data.len())
                } else {
                    // A chunk must be prefixed by chunk size in hexadecimal
                    *this.already_wrote_chunk_size_prefix = true;
                    let total_chunk_size = this
                        .options
                        .chunk_length
                        .or(this.options.stream_length)
                        .unwrap_or_default();
                    prefix_with_total_chunk_size(data, total_chunk_size)
                };

                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                *this.already_wrote_chunk_terminator = true;
                Poll::Ready(Some(Ok(Bytes::from("\r\n0\r\n"))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        // When using aws-chunked content encoding, trailers have to be appended to the body
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        self.already_wrote_trailers
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(
            self.encoded_length()
                .expect("Requests made with aws-chunked encoding must have known size")
                as u64,
        )
    }
}
```

### "The stream is closing early, and I don't know why"

In my early implementation of `http_body::Body` for an `aws-chunked` encoded body, the body wasn't being completely read
out. The problem turned out to be that I was delegating to the `is_end_stream` trait method of the inner body. Because
the innermost body had no knowledge of the trailers I needed to send, it was reporting that the stream had ended.
The fix was to instead rely on the outermost body's knowledge of its own state in order to determine if all data had
been read.

## What helped me to understand the problems and their solutions

- **Reaching out to others that had specific knowledge of a problem:** Talking to a developer that had tackled this
  feature for another SDK was a big help. Special thanks is due to [@jasdel] and the Go v2 SDK team.
  [Their implementation][Go v2 SDK newUnsignedAWSChunkedEncoding] of an `aws-chunked` encoded body was the basis for
  my own implementation.
- **Avoiding codegen**: The process of updating codegen code and then running codegen for each new change you make is
  slow compared to running codegen once at the beginning of development and then just manually editing the generated SDK
  as necessary. I still needed to run `./gradlew :aws:sdk:relocateAwsRuntime :aws:sdk:relocateRuntime` whenever I made
  changes to a runtime crate but that was quick because it's just copying the files. Keep as much code out of codegen as
  possible. It's much easier to modify/debug Rust than it is to write a working codegen module that does the same thing.
  Whenever possible, write the codegen modules later, once the design has settled.
- **Using the `Display` impl for errors:** The `Display` impl for an error can ofter contain helpful info that might not
  be visible when printing with the `Debug` impl. Case in point was an error I was getting because of the
  `is_end_stream` issue. When `Debug` printed, the error looked like this:

  ```rust,ignore
  DispatchFailure(ConnectorError { err: hyper::Error(User(Body), hyper::Error(BodyWriteAborted)), kind: User })
  ```

  That wasn't too helpful for me on its own. I looked into the `hyper` source code and found that the `Display` impl
  contained a helpful message, so I matched into the error and printed the `hyper::Error` with the `Display` impl:

  ```markdown
  user body write aborted: early end, expected 2 more bytes'
  ```

  This helped me understand that I wasn't encoding things correctly and was missing a CRLF.

- **Echo Server**: I first used netcat and then later a small echo server written in Rust to see the raw HTTP request
  being sent out by the SDK as I was working on it. The Rust SDK supports setting endpoints for request. This is often
  used to send requests to something like [LocalStack], but I used it to send request to `localhost` instead:

  ```rust,ignore
  #[tokio::test]
  async fn test_checksum_on_streaming_request_against_s3() {
      let sdk_config = aws_config::from_env()
          .endpoint_resolver(Endpoint::immutable("http://localhost:8080".parse().expect("valid URI")))
          .load().await;
      let s3_client = aws_sdk_s3::Client::new(&sdk_config);

      let input_text = b"Hello world";
      let _res = s3_client
          .put_object()
          .bucket("some-real-bucket")
          .key("test.txt")
          .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
          .checksum_algorithm(ChecksumAlgorithm::Sha256)
          .send()
          .await
          .unwrap();
  }
  ```

  The echo server was based off of an [axum] example and looked like this:

  ```rust,ignore
  use axum::{
    body::{Body, Bytes},
    http::{request::Parts, Request, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::put,
    Router,
  };
  use std::net::SocketAddr;
  use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

  #[tokio::main]
  async fn main() {
    tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::new(
      std::env::var("RUST_LOG").unwrap_or_else(|_| "trace".into()),
    ))
    .with(tracing_subscriber::fmt::layer())
    .init();

    let app = Router::new()
        .route("/", put(|| async move { "200 OK" }))
        .layer(middleware::from_fn(print_request_response));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
  }

  async fn print_request_response(
    req: Request<Body>,
    next: Next<Body>,
  ) -> Result<impl IntoResponse, (StatusCode, String)> {
      let (parts, body) = req.into_parts();

      print_parts(&parts).await;
      let bytes = buffer_and_print("request", body).await?;
      let req = Request::from_parts(parts, Body::from(bytes));

      let res = next.run(req).await;

      Ok(res)
  }

  async fn print_parts(parts: &Parts) {
      tracing::debug!("{:#?}", parts);
  }

  async fn buffer_and_print<B>(direction: &str, body: B) -> Result<Bytes, (StatusCode, String)>
  where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
  {
      let bytes = match hyper::body::to_bytes(body).await {
          Ok(bytes) => bytes,
          Err(err) => {
              return Err((
                  StatusCode::BAD_REQUEST,
                  format!("failed to read {} body: {}", direction, err),
              ));
          }
      };

      if let Ok(body) = std::str::from_utf8(&bytes) {
          tracing::debug!("{} body = {:?}", direction, body);
      }

      Ok(bytes)
  }
  ```
  ##

[S3 Flexible Checksums]: https://aws.amazon.com/blogs/aws/new-additional-checksum-algorithms-for-amazon-s3/
[hyper::error::User::BodyWriteAborted]: https://github.com/hyperium/hyper/blob/740654e55d2bb2f50709f20fb4054a5504d8c2fb/src/error.rs#L98
[HTTP-based operations]: ../transport/operation.md
[an example from the QLDB SDK of creating a body]: https://github.com/awslabs/aws-sdk-rust/blob/1bdfba7f53e77a478f60a1a387e4d9d31fd918fc/sdk/qldbsession/src/input.rs#L197
[MDN maintains a helpful list]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Trailer#directives
[hexadecimal]: https://en.wikipedia.org/wiki/Hexadecimal
[Content-Encoding]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Encoding
[Transfer-Encoding]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Transfer-Encoding
[Go v2 SDK newUnsignedAWSChunkedEncoding]: https://github.com/aws/aws-sdk-go-v2/blob/c214cb61990441aa165e216a3f7e845c50d21939/service/internal/checksum/aws_chunked_encoding.go#L90
[@seanmonstar]: https://github.com/seanmonstar
[@jasdel]: https://github.com/jasdel
[LocalStack]: https://localstack.cloud/
[axum]: https://github.com/tokio-rs/axum
](writing_and_debugging_a_low-level_feature_that_relies_on_HTTP.md)
