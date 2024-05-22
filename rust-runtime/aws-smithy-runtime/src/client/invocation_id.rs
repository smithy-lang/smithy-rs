/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use fastrand::Rng;
use std::fmt::{Debug, Display, Formatter};
use std::sync::{Arc, Mutex};

/// A generator for returning new invocation IDs on demand.
pub trait GenerateInvocationId: Debug + Send + Sync {
    /// Call this method to obtain a new [`InvocationId`] or an error explaining why one couldn't
    /// be provided.
    fn generate(&self) -> Result<Option<InvocationId>, BoxError>;
}

/// Dynamic dispatch implementation of [`GenerateInvocationId`]
#[derive(Clone, Debug)]
pub struct SharedInvocationIdGenerator(Arc<dyn GenerateInvocationId>);

impl SharedInvocationIdGenerator {
    /// Creates a new [`SharedInvocationIdGenerator`].
    pub fn new(gen: impl GenerateInvocationId + 'static) -> Self {
        Self(Arc::new(gen))
    }
}

impl GenerateInvocationId for SharedInvocationIdGenerator {
    fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
        self.0.generate()
    }
}

impl Storable for SharedInvocationIdGenerator {
    type Storer = StoreReplace<Self>;
}

/// `InvocationId` represents a unique ID for each operation invocation per client (retries share the same invocation ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationId(String);

impl InvocationId {
    /// Create an invocation ID with the given value.
    pub fn new(invocation_id: impl Into<String>) -> Self {
        Self(invocation_id.into())
    }
}

impl Display for InvocationId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Storable for InvocationId {
    type Storer = StoreReplace<Self>;
}

/// An invocation ID generator that uses random UUIDs for the invocation ID.
#[derive(Debug, Default)]
pub(crate) struct DefaultInvocationIdGenerator {
    rng: Mutex<Rng>,
}

impl DefaultInvocationIdGenerator {
    /// Creates a new [`DefaultInvocationIdGenerator`].
    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Default::default()
    }
}

impl GenerateInvocationId for DefaultInvocationIdGenerator {
    fn generate(&self) -> Result<Option<InvocationId>, BoxError> {
        let mut rng = self.rng.lock().unwrap();
        let mut random_bytes = [0u8; 16];
        rng.fill(&mut random_bytes);

        let id = uuid::Builder::from_random_bytes(random_bytes).into_uuid();
        Ok(Some(InvocationId::new(id.to_string())))
    }
}
