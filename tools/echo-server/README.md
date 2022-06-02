# echo-server

This is a tool that Zelda created while [implementing the flexible checksums feature]. It's a simple echo server that
will log the requests it receives.

## How to use it

First, start the echo server:

```shell
cargo run
```

Then, configure the SDK to send requests to this server by setting an immutable endpoint when building the `SdkConfig`:

```rust
  #[tokio::test]
  async fn test_checksum_on_streaming_request_against_s3() {
      let sdk_config = aws_config::from_env()
          .endpoint_resolver(Endpoint::immutable("http://localhost:3000".parse().expect("valid URI")))
          .load().await;
      let s3_client = aws_sdk_s3::Client::new(&sdk_config);

      let _res = s3_client
          .put_object()
          .bucket("some-real-bucket")
          .key("test.txt")
          .body(aws_sdk_s3::types::ByteStream::from_static(b"Hello world"))
          .checksum_algorithm(ChecksumAlgorithm::Sha256)
          .send()
          .await
          .unwrap();
  }
```

Once you run your app and start making requests, those requests will be logged by the echo server. The echo server doesn't return valid responses, so you'll have to account for that.

## Acknowledgements

This server is based on the [print-request-response] example from the [axum repo]

[implementing the flexible checksums feature]: ../../design/src/contributing/writing_and_debugging_a_low-level_feature_that_relies_on_HTTP.md
[print-request-response]: https://github.com/tokio-rs/axum/tree/main/examples/print-request-response
[axum repo]: https://github.com/tokio-rs/axum
