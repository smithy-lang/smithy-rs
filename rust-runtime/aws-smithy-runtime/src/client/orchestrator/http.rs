/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Interceptor;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use bytes::{Buf, Bytes};
use http_body::Body;
use pin_utils::pin_mut;
use tracing::trace;

const LOG_SENSITIVE_BODIES: &str = "LOG_SENSITIVE_BODIES";

async fn body_to_bytes(body: SdkBody) -> Result<Bytes, <SdkBody as Body>::Error> {
    let mut output = Vec::new();
    pin_mut!(body);
    while let Some(buf) = body.data().await {
        let mut buf = buf?;
        while buf.has_remaining() {
            output.extend_from_slice(buf.chunk());
            buf.advance(buf.chunk().len())
        }
    }

    Ok(Bytes::from(output))
}

pub(crate) async fn read_body(response: &mut HttpResponse) -> Result<(), <SdkBody as Body>::Error> {
    let mut body = SdkBody::taken();
    std::mem::swap(&mut body, response.body_mut());

    let bytes = body_to_bytes(body).await?;
    let mut body = SdkBody::from(bytes);
    std::mem::swap(&mut body, response.body_mut());

    Ok(())
}

#[derive(Debug)]
pub(crate) struct SensitiveOutput;
impl Storable for SensitiveOutput {
    type Storer = StoreReplace<Self>;
}

/// An interceptor to inject a marker `SensitiveOutput`, causing wire logging of a response body
/// to be disabled.
#[derive(Debug)]
pub struct SensitiveOutputInterceptor;
impl Interceptor for SensitiveOutputInterceptor {
    fn name(&self) -> &'static str {
        "SensitiveOutputInterceptor"
    }

    fn modify_before_deserialization(
        &self,
        _context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        cfg.interceptor_state().store_put(SensitiveOutput);
        Ok(())
    }
}

pub(crate) fn log_response_body(response: &HttpResponse, cfg: &ConfigBag) {
    if cfg.load::<SensitiveOutput>().is_none()
        || std::env::var(LOG_SENSITIVE_BODIES)
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or_default()
    {
        trace!(response = ?response, "read HTTP response body");
    } else {
        trace!(response = "** REDACTED **. To print, set LOG_SENSITIVE_BODIES=true", "read HTTP response body")
    }
}
