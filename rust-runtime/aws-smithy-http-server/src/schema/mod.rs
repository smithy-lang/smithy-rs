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
pub use protocol::{
    collect_request_body, RequestBodyCollectionConfig, RequestBodyCollectionError, ServerEventStreamProtocol,
    ServerProtocol, ServerRequest, ServiceRequestBodyConfig, SharedServerProtocol,
};

use aws_smithy_schema::OperationSchema;

/// The protocol and the operation selected by routing, stored in the request extensions.
///
/// The protocol is erased: everything after routing works through `dyn ServerProtocol`, so a
/// service can select a different protocol per request without anything downstream knowing.
#[derive(Clone)]
pub struct SelectedProtocolOperation {
    protocol: SharedServerProtocol,
    operation: &'static OperationSchema<'static>,
}

impl SelectedProtocolOperation {
    pub fn new(protocol: SharedServerProtocol, operation: &'static OperationSchema<'static>) -> Self {
        Self { protocol, operation }
    }

    /// The selected protocol.
    pub fn protocol(&self) -> &SharedServerProtocol {
        &self.protocol
    }

    /// The routed operation.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.operation
    }
}

impl std::fmt::Debug for SelectedProtocolOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedProtocolOperation")
            .field("protocol", &self.protocol.protocol_id())
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}
