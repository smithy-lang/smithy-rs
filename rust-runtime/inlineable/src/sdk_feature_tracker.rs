/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[allow(dead_code)]
pub(crate) mod rpc_v2_cbor {
    use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextMut;
    use aws_smithy_runtime_api::client::interceptors::Intercept;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
    use aws_smithy_types::config_bag::ConfigBag;

    #[derive(Debug)]
    pub(crate) struct RpcV2CborFeatureTrackerInterceptor;

    impl RpcV2CborFeatureTrackerInterceptor {
        pub(crate) fn new() -> Self {
            Self
        }
    }

    impl Intercept for RpcV2CborFeatureTrackerInterceptor {
        fn name(&self) -> &'static str {
            "RpcV2CborFeatureTrackerInterceptor"
        }

        fn modify_before_serialization(
            &self,
            _context: &mut BeforeSerializationInterceptorContextMut<'_>,
            _runtime_components: &RuntimeComponents,
            cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            cfg.interceptor_state()
                .store_append::<SmithySdkFeature>(SmithySdkFeature::ProtocolRpcV2Cbor);
            Ok(())
        }
    }
}
