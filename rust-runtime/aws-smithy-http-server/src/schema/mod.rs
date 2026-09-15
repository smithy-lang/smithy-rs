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
    collect_for_routing, collect_request_body, parse_settings_json, settings_bool, ProtocolRegistration,
    ProtocolRegistry, RequestBodyCollectionConfig, RequestBodyCollectionError, ServerEventStreamProtocol,
    ServerProtocol, ServerRequest, ServiceRequestBodyConfig, SharedServerProtocol,
};
pub(crate) use protocol::body_collection_rejection;

use aws_smithy_schema::OperationSchema;

/// The protocol and the operation selected by routing, stored in the request extensions.
///
/// The protocol is erased: everything after routing works through `dyn ServerProtocol`.
///
/// The operation schema is stored for consumers that are not generic over the operation:
/// middleware and [`FromParts`](crate::request::FromParts) extractors read this extension to
/// learn which operation was selected — for logging, auth, or metrics — without naming an `Op`
/// type. See the middleware example on [`ServerProtocol`].
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

    /// The routed operation's schema, for code that cannot name the operation type.
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
