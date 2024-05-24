/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use fastrand::Rng;
use http::{HeaderName, HeaderValue};

use aws_smithy_runtime::client::invocation_id::InvocationId as GenericClientInvocationId;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
#[cfg(feature = "test-util")]
pub use test_util::{NoInvocationIdGenerator, PredefinedInvocationIdGenerator};

#[allow(clippy::declare_interior_mutable_const)] // we will never mutate this
const AMZ_SDK_INVOCATION_ID: HeaderName = HeaderName::from_static("amz-sdk-invocation-id");

// We have a trait `aws_smithy_runtime::client::invocation_id::GenerateInvocationId` to
// represent effectively the same concept, but this pub trait was defined first, making it
// challenging to promote it to the `aws_smithy_runtime` crate. We keep this trait here to avoid
// breaking existing users.
/// A generator for returning new invocation IDs on demand.
pub trait InvocationIdGenerator: Debug + Send + Sync {
    /// Call this function to receive a new [`InvocationId`] or an error explaining why one couldn't
    /// be provided.
    fn generate(&self) -> Result<Option<InvocationId>, BoxError>;
}

/// Dynamic dispatch implementation of [`InvocationIdGenerator`]
#[derive(Clone, Debug)]
pub struct SharedInvocationIdGenerator(Arc<dyn InvocationIdGenerator>);

impl SharedInvocationIdGenerator {
    /// Creates a new [`SharedInvocationIdGenerator`].
    pub fn new(gen: impl InvocationIdGenerator + 'static) -> Self {
        Self(Arc::new(gen))
    }
}

impl InvocationIdGenerator for SharedInvocationIdGenerator {
    fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
        self.0.generate()
    }
}

impl Storable for SharedInvocationIdGenerator {
    type Storer = StoreReplace<Self>;
}

/// An invocation ID generator that uses random UUIDs for the invocation ID.
#[deprecated(
    since = "1.2.3",
    note = "This struct was meant to be used internally. Do not use."
)]
#[derive(Debug, Default)]
pub struct DefaultInvocationIdGenerator {
    rng: Mutex<Rng>,
}

#[allow(deprecated)]
impl DefaultInvocationIdGenerator {
    /// Creates a new [`DefaultInvocationIdGenerator`].
    pub fn new() -> Self {
        Default::default()
    }

    /// Creates a [`DefaultInvocationIdGenerator`] with the given seed.
    pub fn with_seed(seed: u64) -> Self {
        Self {
            rng: Mutex::new(Rng::with_seed(seed)),
        }
    }
}

#[allow(deprecated)]
impl InvocationIdGenerator for DefaultInvocationIdGenerator {
    fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
        let mut rng = self.rng.lock().unwrap();
        let mut random_bytes = [0u8; 16];
        rng.fill(&mut random_bytes);

        let id = uuid::Builder::from_random_bytes(random_bytes).into_uuid();
        Ok(Some(InvocationId::new(id.to_string())))
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

impl Intercept for InvocationIdInterceptor {
    fn name(&self) -> &'static str {
        "InvocationIdInterceptor"
    }

    fn modify_before_retry_loop(
        &self,
        _ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(gen) = cfg.load::<SharedInvocationIdGenerator>() {
            match InvocationIdGenerator::generate(gen)? {
                Some(id) => {
                    cfg.interceptor_state()
                        .store_put::<GenericClientInvocationId>(GenericClientInvocationId::new(
                            id.0,
                        ));
                }
                None => {
                    cfg.interceptor_state()
                        .store_or_unset::<GenericClientInvocationId>(None);
                }
            }
        }

        Ok(())
    }

    fn modify_before_transmit(
        &self,
        ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let headers = ctx.request_mut().headers_mut();
        if let Some(id) = cfg.load::<GenericClientInvocationId>() {
            headers.append(
                AMZ_SDK_INVOCATION_ID,
                HeaderValue::try_from(id.to_string())?,
            );
        }
        Ok(())
    }
}

/// InvocationId provides a consistent ID across retries
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationId(String);

impl InvocationId {
    /// Create an invocation ID with the given value.
    pub fn new(invocation_id: String) -> Self {
        Self(invocation_id)
    }
}

impl Storable for InvocationId {
    type Storer = StoreReplace<Self>;
}

#[cfg(feature = "test-util")]
mod test_util {
    use aws_smithy_runtime::client::invocation_id::GenerateInvocationId;
    use std::sync::{Arc, Mutex};

    use super::*;

    impl InvocationId {
        /// Create a new invocation ID from a `&'static str`.
        pub fn new_from_str(uuid: &'static str) -> Self {
            Self(uuid.to_owned())
        }
    }

    /// A "generator" that returns [`InvocationId`]s from a predefined list.
    #[derive(Debug)]
    pub struct PredefinedInvocationIdGenerator {
        pre_generated_ids: Arc<Mutex<Vec<InvocationId>>>,
    }

    impl PredefinedInvocationIdGenerator {
        /// Given a `Vec<InvocationId>`, create a new [`PredefinedInvocationIdGenerator`].
        pub fn new(mut invocation_ids: Vec<InvocationId>) -> Self {
            // We're going to pop ids off of the end of the list, so we need to reverse the list or else
            // we'll be popping the ids in reverse order, confusing the poor test writer.
            invocation_ids.reverse();

            Self {
                pre_generated_ids: Arc::new(Mutex::new(invocation_ids)),
            }
        }
    }

    impl InvocationIdGenerator for PredefinedInvocationIdGenerator {
        fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
            Ok(Some(
                self.pre_generated_ids
                    .lock()
                    .expect("this will never be under contention")
                    .pop()
                    .expect("testers will provide enough invocation IDs"),
            ))
        }
    }

    impl GenerateInvocationId for PredefinedInvocationIdGenerator {
        fn generate(&self) -> Result<Option<GenericClientInvocationId>, BoxError> {
            InvocationIdGenerator::generate(self)
                .map(|id| id.map(|id| GenericClientInvocationId::new(id.0)))
        }
    }

    /// A "generator" that always returns `None`.
    #[derive(Debug, Default)]
    pub struct NoInvocationIdGenerator;

    impl NoInvocationIdGenerator {
        /// Create a new [`NoInvocationIdGenerator`].
        pub fn new() -> Self {
            Self
        }
    }

    impl InvocationIdGenerator for NoInvocationIdGenerator {
        fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
            Ok(None)
        }
    }

    impl GenerateInvocationId for NoInvocationIdGenerator {
        fn generate(&self) -> Result<Option<GenericClientInvocationId>, BoxError> {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "test-util")]
    #[test]
    fn custom_id_generator() {
        use super::*;
        use aws_smithy_runtime_api::client::interceptors::context::{
            BeforeTransmitInterceptorContextMut, Input, InterceptorContext,
        };
        use aws_smithy_runtime_api::client::interceptors::Intercept;
        use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
        use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
        use aws_smithy_types::config_bag::ConfigBag;
        use aws_smithy_types::config_bag::Layer;

        fn expect_header<'a>(
            context: &'a BeforeTransmitInterceptorContextMut<'_>,
            header_name: &str,
        ) -> &'a str {
            context.request().headers().get(header_name).unwrap()
        }

        let rc = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.enter_serialization_phase();
        ctx.set_request(HttpRequest::empty());
        let _ = ctx.take_input();
        ctx.enter_before_transmit_phase();

        let mut cfg = ConfigBag::base();
        let mut layer = Layer::new("test");
        let gen = PredefinedInvocationIdGenerator::new(vec![InvocationId::new(
            "the-best-invocation-id".into(),
        )]);
        layer.store_put(GenericClientInvocationId::new(
            gen.generate().unwrap().unwrap().0,
        ));
        cfg.push_layer(layer);

        let interceptor = InvocationIdInterceptor::new();
        let mut ctx = Into::into(&mut ctx);
        interceptor
            .modify_before_transmit(&mut ctx, &rc, &mut cfg)
            .unwrap();

        let header = expect_header(&ctx, "amz-sdk-invocation-id");
        assert_eq!("the-best-invocation-id", header);
    }
}
