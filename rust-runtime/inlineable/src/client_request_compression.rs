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

type CustomDefaultFn = Box<
    dyn Fn(Option<CompressionAlgorithm>) -> Option<CompressionAlgorithm> + Send + Sync + 'static,
>;

pub(crate) struct DefaultRequestCompressionOverride {
    custom_default: CustomDefaultFn,
}

impl fmt::Debug for DefaultRequestCompressionOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultRequestCompressionOverride").finish()
    }
}

impl Storable for DefaultRequestCompressionOverride {
    type Storer = StoreReplace<Self>;
}

impl DefaultRequestCompressionOverride {
    pub(crate) fn new<F>(custom_default: F) -> Self
    where
        F: Fn(Option<CompressionAlgorithm>) -> Option<CompressionAlgorithm> + Send + Sync + 'static,
    {
        Self {
            custom_default: Box::new(custom_default),
        }
    }

    pub(crate) fn custom_default(
        &self,
        original: Option<CompressionAlgorithm>,
    ) -> Option<CompressionAlgorithm> {
        (self.custom_default)(original)
    }
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
        let options = cfg
            .load::<CompressionOptions>()
            .cloned()
            .unwrap_or_default();

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
        let algorithm = incorporate_custom_default(Some(CompressionAlgorithm::Gzip), cfg);
        if let Some(algorithm) = algorithm {
            let request = context.request_mut();
            wrap_request_body_in_compressed_body(request, algorithm, &options)?;
        }

        Ok(())
    }
}

fn incorporate_custom_default(
    algorithm: Option<CompressionAlgorithm>,
    cfg: &ConfigBag,
) -> Option<CompressionAlgorithm> {
    cfg.load::<DefaultRequestCompressionOverride>()
        .and_then(|it| it.custom_default(algorithm))
        .or(algorithm)
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
