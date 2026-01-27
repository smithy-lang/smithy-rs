/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, UNIX_EPOCH};

use aws_runtime::auth::PayloadSigningOverride;
use aws_runtime::content_encoding::{AwsChunkedBodyOptions, DeferredSigner};
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{Client, Config};
use aws_smithy_async::test_util::ManualTimeSource;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_http_client::test_util::dvr::ReplayingClient;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use bytes::Bytes;
use http_body_1x::{Body, SizeHint};
use pin_project_lite::pin_project;

// Interceptor that forces chunk signing for testing purposes.
//
// Chunk signing during AWS chunked content encoding only occurs when requests are sent
// without TLS. This interceptor overrides the `AwsChunkedContentEncodingInterceptor`
// configuration to enable chunk signing regardless of transport security.
#[derive(Debug)]
struct ForceChunkedSigningInterceptor {
    time_source: ManualTimeSource,
}

impl Intercept for ForceChunkedSigningInterceptor {
    fn name(&self) -> &'static str {
        "ForceChunkedSigningInterceptor"
    }

    fn modify_before_signing(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Grab existing options and update them with signing enabled
        let chunked_body_options = cfg
            .get_mut_from_interceptor_state::<AwsChunkedBodyOptions>()
            .expect("AwsChunkedBodyOptions should be set");

        let chunked_body_options = std::mem::take(chunked_body_options)
            .signed_chunked_encoding(true)
            .with_trailer_len(("x-amz-trailer-signature".len() + ":".len() + 64) as u64);

        cfg.interceptor_state().store_put(chunked_body_options);

        let (signer, sender) = DeferredSigner::new();
        cfg.interceptor_state().store_put(signer);
        cfg.interceptor_state().store_put(sender);

        cfg.interceptor_state()
            .store_put(PayloadSigningOverride::StreamingSignedPayloadTrailer);

        Ok(())
    }

    // Verifies the chunk signer uses a `StaticTimeSource` by advancing time by 1 second
    // before transmission. If a dynamic time source were used, the test would fail with
    // a chunk signature mismatch.
    fn modify_before_transmit(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.time_source.advance(Duration::from_secs(1));
        Ok(())
    }
}

// Custom streaming body
pin_project! {
    struct TestBody {
        data: Option<Bytes>,
    }
}

impl Body for TestBody {
    type Data = Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        if let Some(data) = this.data.take() {
            return Poll::Ready(Some(Ok(http_body_1x::Frame::data(data))));
        }

        Poll::Ready(None)
    }

    fn size_hint(&self) -> SizeHint {
        let mut size = SizeHint::default();
        size.set_lower(self.data.as_ref().map_or(0, |d| d.len() as u64));
        size.set_upper(self.data.as_ref().map_or(0, |d| d.len() as u64));
        size
    }
}

#[tokio::test]
async fn test_signing_for_aws_chunked_content_encoding() {
    let time_source = ManualTimeSource::new(UNIX_EPOCH + Duration::from_secs(1234567890));

    let http_client = ReplayingClient::from_file("tests/data/chunk_signing/chunk-signing.json")
        .expect("recorded HTTP communication exists");
    let config = Config::builder()
        .with_test_defaults()
        .http_client(http_client.clone())
        .region(Region::new("us-east-1"))
        .time_source(SharedTimeSource::new(time_source.clone()))
        .build();

    let client = Client::from_conf(config);

    let bucket = "test-bucket";

    // 65 KB of 'a' characters. With a 64 KB chunk size, the payload splits into four chunks:
    // 64 KB, 1 KB, 0 bytes, and the final chunk containing trailing headers.
    let data = "a".repeat(65 * 1024);
    let body = TestBody {
        data: Some(Bytes::from(data)),
    };
    let body = ByteStream::from_body_1_x(body);

    let _ = dbg!(client
        .put_object()
        .body(body)
        .bucket(bucket)
        .key("mytest.txt")
        .customize()
        .config_override(
            Config::builder().interceptor(ForceChunkedSigningInterceptor { time_source })
        )
        .send()
        .await
        .unwrap());

    // Verify content-encoding is `aws-chunked` and all chunk signatures match.
    http_client
        .validate_body_and_headers(Some(&["content-encoding"]), "application/octet-stream")
        .await
        .unwrap();
}
