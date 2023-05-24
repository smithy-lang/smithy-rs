/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::interceptors::error::BoxError;
use aws_smithy_runtime_api::client::interceptors::{
    BeforeTransmitInterceptorContextMut, Interceptor,
};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use http::{HeaderName, HeaderValue};
use std::fmt::Debug;
use uuid::Uuid;

#[cfg(feature = "test-util")]
use std::sync::{Arc, Mutex};

#[allow(clippy::declare_interior_mutable_const)] // we will never mutate this
const AMZ_SDK_INVOCATION_ID: HeaderName = HeaderName::from_static("amz-sdk-invocation-id");

/// A generator for returning new invocation IDs on demand.
pub trait InvocationIdGenerator: Debug + Send + Sync {
    /// Call this function to receive a new [`InvocationId`] or an error explaining why one couldn't
    /// be provided.
    fn generate(&self) -> Result<InvocationId, BoxError>;
}

/// A generator for random invocation IDs. Invocation IDs are shared across all attempts made in
/// service of an operation and are used to help Services track requests.
#[derive(Debug, Default)]
pub struct RandomInvocationIdGenerator {}

impl RandomInvocationIdGenerator {
    /// Create a new `RandomInvocationIdGenerator`.
    pub fn new() -> Self {
        Self {}
    }
}

impl InvocationIdGenerator for RandomInvocationIdGenerator {
    fn generate(&self) -> Result<InvocationId, BoxError> {
        Ok(InvocationId::new())
    }
}

/// A "generator" that returns [`InvocationId`]s from a predefined list.
#[cfg(feature = "test-util")]
#[derive(Debug)]
pub struct InvocationIdGeneratorForTests {
    pre_generated_ids: Arc<Mutex<Vec<InvocationId>>>,
}

#[cfg(feature = "test-util")]
impl InvocationIdGeneratorForTests {
    /// Given a `Vec<InvocationId>`, create a new [`InvocationIdGeneratorForTests`].
    pub fn new(mut invocation_ids: Vec<InvocationId>) -> Self {
        // We're going to pop ids off of the end of the list, so we need to reverse the list or else
        // we'll be popping the ids in reverse order, confusing the poor test writer.
        invocation_ids.reverse();

        Self {
            pre_generated_ids: Arc::new(Mutex::new(invocation_ids)),
        }
    }
}

#[cfg(feature = "test-util")]
impl InvocationIdGenerator for InvocationIdGeneratorForTests {
    fn generate(&self) -> Result<InvocationId, BoxError> {
        Ok(self
            .pre_generated_ids
            .lock()
            .expect("this will never be under contention")
            .pop()
            .expect("testers will provide enough invocation IDs"))
    }
}

/// This interceptor generates a UUID and attaches it to all request attempts made as part of this operation.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct InvocationIdInterceptor {}

impl InvocationIdInterceptor {
    /// Creates a new `InvocationIdInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for InvocationIdInterceptor {
    fn modify_before_retry_loop(
        &self,
        _ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let gen = cfg
            .get::<Box<dyn InvocationIdGenerator>>()
            .ok_or("Expected an InvocationIdGenerator in the ConfigBag but none was present")?;
        let id = gen.generate()?;
        cfg.put::<InvocationId>(id);
        Ok(())
    }

    fn modify_before_transmit(
        &self,
        ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let headers = ctx.request_mut().headers_mut();
        let id = cfg
            .get::<InvocationId>()
            .ok_or("Expected an InvocationId in the ConfigBag but none was present")?;
        headers.append(AMZ_SDK_INVOCATION_ID, id.0.clone());
        Ok(())
    }
}

/// InvocationId provides a consistent ID across retries
#[derive(Debug, Clone)]
pub struct InvocationId(HeaderValue);

impl InvocationId {
    /// Create a new, random, invocation ID.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new invocation ID from a `&'static str`.
    #[cfg(feature = "test-util")]
    pub fn new_from_str(uuid: &'static str) -> Self {
        InvocationId(HeaderValue::from_static(uuid))
    }
}

impl Default for InvocationId {
    fn default() -> Self {
        let id = Uuid::new_v4();
        let id = id
            .to_string()
            .parse()
            .expect("UUIDs always produce a valid header value");
        Self(id)
    }
}

#[cfg(all(test, feature = "test-util"))]
mod tests {
    use super::{InvocationId, InvocationIdGeneratorForTests, InvocationIdInterceptor};
    use crate::invocation_id::InvocationIdGenerator;
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext};
    use aws_smithy_runtime_api::config_bag::ConfigBag;
    use aws_smithy_runtime_api::type_erasure::TypedBox;
    use http::HeaderValue;

    fn expect_header<'a>(context: &'a InterceptorContext, header_name: &str) -> &'a HeaderValue {
        context
            .request()
            .expect("request is set")
            .headers()
            .get(header_name)
            .unwrap()
    }

    #[test]
    fn test_id_is_generated_and_set() {
        let mut context = InterceptorContext::new(TypedBox::new("doesntmatter").erase());
        context.enter_serialization_phase();
        context.set_request(http::Request::builder().body(SdkBody::empty()).unwrap());
        let _ = context.take_input();
        context.enter_before_transmit_phase();

        let mut config = ConfigBag::base();
        let id_gen = InvocationIdGeneratorForTests::new(vec![InvocationId::new_from_str(
            "367dc4d4-ae64-49a8-a1b3-d40226de0f95",
        )]);
        config.put::<Box<dyn InvocationIdGenerator>>(Box::new(id_gen));
        let interceptor = InvocationIdInterceptor::new();
        let mut ctx = Into::into(&mut context);
        interceptor
            .modify_before_signing(&mut ctx, &mut config)
            .unwrap();
        interceptor
            .modify_before_retry_loop(&mut ctx, &mut config)
            .unwrap();

        let header = expect_header(&context, "amz-sdk-invocation-id");
        assert_eq!("367dc4d4-ae64-49a8-a1b3-d40226de0f95", header);
        // UUID should include 32 chars and 4 dashes
        assert_eq!(header.len(), 36);
    }
}
