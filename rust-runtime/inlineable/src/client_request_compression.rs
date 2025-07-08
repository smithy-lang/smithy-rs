/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_compression::body::compress::CompressedBody;
use aws_smithy_compression::http::http_body_1_x::CompressRequest;
use aws_smithy_compression::{CompressionAlgorithm, CompressionOptions};
use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
};
use aws_smithy_runtime_api::client::interceptors::{Intercept, SharedInterceptor};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::{ConfigBag, Layer, Storable, StoreReplace};
use aws_smithy_types::error::operation::BuildError;
use std::borrow::Cow;
use std::{fmt, mem};

#[derive(Debug)]
pub(crate) struct RequestCompressionRuntimePlugin {
    runtime_components: RuntimeComponentsBuilder,
}

impl RequestCompressionRuntimePlugin {
    pub(crate) fn new() -> Self {
        Self {
            runtime_components: RuntimeComponentsBuilder::new("RequestCompressionRuntimePlugin")
                .with_interceptor(SharedInterceptor::new(RequestCompressionInterceptor::new())),
        }
    }
}

impl RuntimePlugin for RequestCompressionRuntimePlugin {
    fn runtime_components(
        &self,
        _: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        Cow::Borrowed(&self.runtime_components)
    }
}

#[derive(Debug)]
struct RequestCompressionInterceptorState {
    options: Option<CompressionOptions>,
}

impl Storable for RequestCompressionInterceptorState {
    type Storer = StoreReplace<Self>;
}

/// Interceptor for Smithy [`@requestCompression`][spec].
///
/// [spec]: https://smithy.io/2.0/spec/behavior-traits.html#requestcompression-trait
pub(crate) struct RequestCompressionInterceptor {}

impl fmt::Debug for RequestCompressionInterceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestCompressionInterceptor").finish()
    }
}

impl RequestCompressionInterceptor {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl Intercept for RequestCompressionInterceptor {
    fn name(&self) -> &'static str {
        "RequestCompressionInterceptor"
    }

    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let disable_request_compression = cfg
            .load::<DisableRequestCompression>()
            .cloned()
            .unwrap_or_default();
        let request_min_compression_size_bytes = cfg
            .load::<RequestMinCompressionSizeBytes>()
            .cloned()
            .unwrap_or_default();
        let options = CompressionOptions::default()
            .with_min_compression_size_bytes(request_min_compression_size_bytes.0)?
            .with_enabled(!disable_request_compression.0);

        let mut layer = Layer::new("RequestCompressionInterceptor");
        layer.store_put(RequestCompressionInterceptorState {
            options: Some(options),
        });

        cfg.push_layer(layer);

        Ok(())
    }

    fn modify_before_retry_loop(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let state = cfg
            .load::<RequestCompressionInterceptorState>()
            .expect("set in `read_before_execution`");

        let options = state.options.clone().unwrap();
        let request = context.request_mut();

        // Don't wrap a body if compression is disabled.
        if !options.is_enabled() {
            tracing::trace!("request compression is disabled and will not be applied");
            return Ok(());
        }

        // Don't wrap a body if it's below the minimum size
        //
        // Because compressing small amounts of data can actually increase its size,
        // we check to see if the data is big enough to make compression worthwhile.
        let size_hint = http_body_1x::Body::size_hint(request.body()).exact();
        if let Some(known_size) = size_hint {
            if known_size < options.min_compression_size_bytes() as u64 {
                tracing::trace!(
                    min_compression_size_bytes = options.min_compression_size_bytes(),
                    known_size,
                    "request body is below minimum size and will not be compressed"
                );
                return Ok(());
            }
            tracing::trace!("compressing sized request body...");
        } else {
            tracing::trace!("compressing unsized request body...");
        }

        wrap_request_body_in_compressed_body(
            request,
            CompressionAlgorithm::Gzip.into_impl_http_body_1_x(&options),
        )?;
        cfg.interceptor_state()
            .store_append::<SmithySdkFeature>(SmithySdkFeature::GzipRequestCompression);

        Ok(())
    }
}

fn wrap_request_body_in_compressed_body(
    request: &mut HttpRequest,
    request_compress_impl: Box<dyn CompressRequest>,
) -> Result<(), BuildError> {
    request.headers_mut().append(
        request_compress_impl.header_name(),
        request_compress_impl.header_value(),
    );
    let mut body = {
        let body = mem::replace(request.body_mut(), SdkBody::taken());

        if body.is_streaming() {
            request
                .headers_mut()
                .remove(http_1x::header::CONTENT_LENGTH);
            body.map(move |body| {
                let body = CompressedBody::new(body, request_compress_impl.clone());
                SdkBody::from_body_1_x(body)
            })
        } else {
            let body = CompressedBody::new(body, request_compress_impl.clone());
            let body = body.into_compressed_sdk_body().map_err(BuildError::other)?;

            let content_length = body.content_length().expect("this payload is in-memory");
            request
                .headers_mut()
                .insert(http_1x::header::CONTENT_LENGTH, content_length.to_string());

            body
        }
    };
    mem::swap(request.body_mut(), &mut body);

    Ok(())
}

#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct DisableRequestCompression(pub(crate) bool);

impl From<bool> for DisableRequestCompression {
    fn from(value: bool) -> Self {
        DisableRequestCompression(value)
    }
}

impl Storable for DisableRequestCompression {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct RequestMinCompressionSizeBytes(pub(crate) u32);

impl Default for RequestMinCompressionSizeBytes {
    fn default() -> Self {
        RequestMinCompressionSizeBytes(10240)
    }
}

impl From<u32> for RequestMinCompressionSizeBytes {
    fn from(value: u32) -> Self {
        RequestMinCompressionSizeBytes(value)
    }
}

impl Storable for RequestMinCompressionSizeBytes {
    type Storer = StoreReplace<Self>;
}

#[cfg(test)]
mod tests {
    use super::wrap_request_body_in_compressed_body;
    use crate::client_request_compression::{
        RequestCompressionInterceptor, RequestMinCompressionSizeBytes,
    };
    use aws_smithy_compression::{CompressionAlgorithm, CompressionOptions};
    use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
    use aws_smithy_runtime_api::client::interceptors::context::{Input, InterceptorContext};
    use aws_smithy_runtime_api::client::interceptors::Intercept;
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::config_bag::{ConfigBag, Layer};
    use http_body_1x::Body;
    use http_body_util::BodyExt;

    const UNCOMPRESSED_INPUT: &[u8] = b"hello world";
    const COMPRESSED_OUTPUT: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0,
        133, 17, 74, 13, 11, 0, 0, 0,
    ];

    #[tokio::test]
    async fn test_compressed_body_is_retryable() {
        let mut request: HttpRequest = http_1x::Request::builder()
            .body(SdkBody::retryable(move || {
                SdkBody::from(UNCOMPRESSED_INPUT)
            }))
            .unwrap()
            .try_into()
            .unwrap();

        // ensure original SdkBody is retryable
        let mut body = request.body().try_clone().unwrap();
        let mut body_data = Vec::new();
        while let Some(Ok(frame)) = body.frame().await {
            let data = frame.into_data().expect("Data frame");
            body_data.extend_from_slice(&data)
        }
        // Not yet wrapped, should still be the same as UNCOMPRESSED_INPUT.
        assert_eq!(UNCOMPRESSED_INPUT, body_data);

        let compression_algorithm = CompressionAlgorithm::Gzip;
        let compression_options = CompressionOptions::default()
            .with_min_compression_size_bytes(0)
            .unwrap();

        wrap_request_body_in_compressed_body(
            &mut request,
            compression_algorithm.into_impl_http_body_1_x(&compression_options),
        )
        .unwrap();

        // ensure again that wrapped SdkBody is retryable
        let mut body = request.body().try_clone().expect("body is retryable");
        let mut body_data = Vec::new();
        while let Some(Ok(frame)) = body.frame().await {
            let data = frame.into_data().expect("Data frame");
            body_data.extend_from_slice(&data)
        }

        // Since this body was wrapped, the output should be compressed data
        assert_ne!(UNCOMPRESSED_INPUT, body_data.as_slice());
        assert_eq!(COMPRESSED_OUTPUT, body_data.as_slice());
    }

    fn context() -> InterceptorContext {
        let mut context = InterceptorContext::new(Input::doesnt_matter());
        context.enter_serialization_phase();
        context.set_request(
            http_1x::Request::builder()
                .body(SdkBody::from(UNCOMPRESSED_INPUT))
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let _ = context.take_input();
        context.enter_before_transmit_phase();
        context
    }

    #[tokio::test]
    async fn test_sdk_feature_gzip_request_compression_should_be_tracked() {
        let mut cfg = ConfigBag::base();
        let mut layer = Layer::new("test");
        layer.store_put(RequestMinCompressionSizeBytes::from(0));
        cfg.push_layer(layer);
        let mut context = context();
        let ctx = Into::into(&context);

        let sut = RequestCompressionInterceptor::new();
        sut.read_before_execution(&ctx, &mut cfg).unwrap();

        let rc = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let mut ctx = Into::into(&mut context);
        sut.modify_before_retry_loop(&mut ctx, &rc, &mut cfg)
            .unwrap();

        assert_eq!(
            &SmithySdkFeature::GzipRequestCompression,
            cfg.load::<SmithySdkFeature>().next().unwrap()
        );
    }
}
