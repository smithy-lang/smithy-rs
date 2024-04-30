/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;
use std::{fmt, mem};

use http::HeaderValue;

use aws_smithy_compression::body::compress::CompressedBody;
use aws_smithy_compression::{CompressionAlgorithm, CompressionOptions};
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

/// Errors related to constructing compression-validated HTTP requests
#[derive(Debug)]
pub(crate) enum Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error during request compression")
    }
}

impl std::error::Error for Error {}

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

    fn read_before_serialization(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
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
            .with_disabled(disable_request_compression.0);

        let mut layer = Layer::new("RequestCompressionInterceptor");
        layer.store_put(RequestCompressionInterceptorState {
            options: Some(options),
        });
        cfg.push_layer(layer);

        Ok(())
    }

    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let state = cfg
            .load::<RequestCompressionInterceptorState>()
            .expect("set in `read_before_serialization`");

        let options = state.options.clone().unwrap();
        let request = context.request_mut();
        wrap_request_body_in_compressed_body(request, CompressionAlgorithm::Gzip, &options)?;

        Ok(())
    }
}

fn wrap_request_body_in_compressed_body(
    request: &mut HttpRequest,
    compression_algorithm: CompressionAlgorithm,
    compression_options: &CompressionOptions,
) -> Result<(), BuildError> {
    let mut body = {
        let body = mem::replace(request.body_mut(), SdkBody::taken());

        let options = compression_options.clone();
        body.map(move |body| {
            let body = CompressedBody::new(body, compression_algorithm.into_impl(&options));

            SdkBody::from_body_0_4(body)
        })
    };

    let headers = request.headers_mut();
    headers.insert(
        http::header::CONTENT_ENCODING,
        HeaderValue::from_str("gzip")
            .map_err(BuildError::other)
            .expect("\"gzip\" will always be a valid HeaderValue"),
    );

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
    use http_body::Body;

    use aws_smithy_compression::{CompressionAlgorithm, CompressionOptions};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_types::body::SdkBody;

    use super::wrap_request_body_in_compressed_body;

    const UNCOMPRESSED_INPUT: &[u8] = b"hello world";
    const COMPRESSED_OUTPUT: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0,
        133, 17, 74, 13, 11, 0, 0, 0,
    ];

    #[tokio::test]
    async fn test_compressed_body_is_retryable() {
        let mut request: HttpRequest = http::Request::builder()
            .body(SdkBody::retryable(move || {
                SdkBody::from(UNCOMPRESSED_INPUT)
            }))
            .unwrap()
            .try_into()
            .unwrap();

        // ensure original SdkBody is retryable
        let mut body = request.body().try_clone().unwrap();
        let mut body_data = Vec::new();
        while let Some(data) = body.data().await {
            body_data.extend_from_slice(&data.unwrap())
        }
        // Not yet wrapped, should still be the same as UNCOMPRESSED_INPUT.
        assert_eq!(UNCOMPRESSED_INPUT, body_data);

        let compression_algorithm = CompressionAlgorithm::Gzip;
        let compression_options = CompressionOptions::default();
        wrap_request_body_in_compressed_body(
            &mut request,
            compression_algorithm,
            &compression_options,
        )
        .unwrap();

        // ensure again that wrapped SdkBody is retryable
        let mut body = request.body().try_clone().expect("body is retryable");
        let mut body_data = Vec::new();
        while let Some(data) = body.data().await {
            body_data.extend_from_slice(&data.unwrap())
        }

        // Since this body was wrapped, the output should be compressed data
        assert_ne!(UNCOMPRESSED_INPUT, body_data.as_slice());
        assert_eq!(COMPRESSED_OUTPUT, body_data.as_slice());
    }
}
