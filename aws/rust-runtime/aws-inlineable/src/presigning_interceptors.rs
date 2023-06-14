/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

use crate::presigning::PresigningConfig;
use crate::serialization_settings::HeaderSerializationSettings;
use aws_runtime::auth::sigv4::{HttpSignatureType, SigV4OperationSigningConfig};
use aws_runtime::invocation_id::InvocationIdInterceptor;
use aws_runtime::request_info::RequestInfoInterceptor;
use aws_runtime::user_agent::UserAgentInterceptor;
use aws_sigv4::http_request::SignableBody;
use aws_smithy_async::time::{SharedTimeSource, StaticTimeSource};
use aws_smithy_runtime_api::client::interceptors::{
    disable_interceptor, BeforeSerializationInterceptorContextMut,
    BeforeTransmitInterceptorContextMut, BoxError, Interceptor, InterceptorRegistrar,
    SharedInterceptor,
};
use aws_smithy_runtime_api::client::orchestrator::ConfigBagAccessors;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Layer};

/// Interceptor that tells the SigV4 signer to add the signature to query params,
/// and sets the request expiration time from the presigning config.
#[derive(Debug)]
pub(crate) struct SigV4PresigningInterceptor {
    config: PresigningConfig,
    payload_override: SignableBody<'static>,
}

impl SigV4PresigningInterceptor {
    pub(crate) fn new(config: PresigningConfig, payload_override: SignableBody<'static>) -> Self {
        Self {
            config,
            payload_override,
        }
    }
}

impl Interceptor for SigV4PresigningInterceptor {
    fn modify_before_serialization(
        &self,
        _context: &mut BeforeSerializationInterceptorContextMut<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        cfg.interceptor_state().put::<HeaderSerializationSettings>(
            HeaderSerializationSettings::new()
                .omit_default_content_length()
                .omit_default_content_type(),
        );
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(StaticTimeSource::new(
                self.config.start_time(),
            )));
        Ok(())
    }

    fn modify_before_signing(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(mut config) = cfg.get::<SigV4OperationSigningConfig>().cloned() {
            config.signing_options.expires_in = Some(self.config.expires());
            config.signing_options.signature_type = HttpSignatureType::HttpRequestQueryParams;
            config.signing_options.payload_override = Some(self.payload_override.clone());
            cfg.interceptor_state()
                .put::<SigV4OperationSigningConfig>(config);
            Ok(())
        } else {
            Err(
                "SigV4 presigning requires the SigV4OperationSigningConfig to be in the config bag. \
                This is a bug. Please file an issue.".into(),
            )
        }
    }
}

/// Runtime plugin that registers the SigV4PresigningInterceptor.
#[derive(Debug)]
pub(crate) struct SigV4PresigningRuntimePlugin {
    interceptor: SharedInterceptor,
}

impl SigV4PresigningRuntimePlugin {
    pub(crate) fn new(config: PresigningConfig, payload_override: SignableBody<'static>) -> Self {
        Self {
            interceptor: SharedInterceptor::new(SigV4PresigningInterceptor::new(
                config,
                payload_override,
            )),
        }
    }
}

impl RuntimePlugin for SigV4PresigningRuntimePlugin {
    fn config(&self) -> Option<FrozenLayer> {
        let mut layer = Layer::new("Presigning");
        layer.put(disable_interceptor::<InvocationIdInterceptor>("presigning"));
        layer.put(disable_interceptor::<RequestInfoInterceptor>("presigning"));
        layer.put(disable_interceptor::<UserAgentInterceptor>("presigning"));
        Some(layer.freeze())
    }

    fn interceptors(&self, interceptors: &mut InterceptorRegistrar) {
        interceptors.register(self.interceptor.clone());
    }
}
