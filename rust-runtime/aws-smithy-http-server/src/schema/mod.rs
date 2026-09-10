/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod deserialize;
mod modeled_error;
pub mod protocol;
pub(crate) mod request_bindings;
pub(crate) mod response_bindings;

pub use deserialize::{DeserializableShape, DeserializeError};
pub use modeled_error::{HttpModeledError, ModeledError};
use std::sync::Arc;

/// The protocol and protocol-specific operation state selected by routing.
#[derive(Clone)]
pub struct SelectedProtocolOperation {
    pub(crate) protocol: Arc<dyn DynServerProtocol>,
    pub(crate) operation: Arc<dyn ErasedCompiledOperation>,
}

impl SelectedProtocolOperation {
    pub fn new(protocol: Arc<dyn DynServerProtocol>, operation: Arc<dyn ErasedCompiledOperation>) -> Self {
        Self { protocol, operation }
    }

    pub fn protocol(&self) -> &Arc<dyn DynServerProtocol> {
        &self.protocol
    }
    pub fn operation(&self) -> &Arc<dyn ErasedCompiledOperation> {
        &self.operation
    }
}
pub use protocol::{
    collect_request_body, CompileOperationState, CompiledOperation, DynServerProtocol, ErasedCompiledOperation,
    OperationState, ProtocolRoutingTable, RequestBodyCollectionConfig, RequestBodyCollectionError, RequestBodyHandling,
    ServerProtocol, ServerRequest, ServiceRequestBodyConfig,
};
