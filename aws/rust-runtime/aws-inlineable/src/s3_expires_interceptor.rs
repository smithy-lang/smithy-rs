/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;

#[derive(Debug)]
pub(crate) struct S3ExpiresInterceptor;

impl Intercept for S3ExpiresInterceptor {
    fn name(&self) -> &'static str {
        "S3ExpiresInterceptor"
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        _: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        //Copy the value of the Expires header to the ExpiresString header so
        //that it will be parsed as a String
        let headers = context.response_mut().headers_mut();
        let expires_header = headers.get("Expires").unwrap_or("").to_string();
        headers.insert("ExpiresString", expires_header);

        Ok(())
    }
}
