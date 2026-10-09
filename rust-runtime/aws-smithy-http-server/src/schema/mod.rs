/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod config;
mod deserialize;
pub mod event_stream;
mod modeled_error;
pub mod protocol;
pub(crate) mod request_bindings;
pub(crate) mod response_bindings;
pub mod routing;
mod service;
pub mod settings;
mod timestamp;

pub use config::ServiceConfig;
pub use deserialize::{DeserializableShape, DeserializeError};
pub use event_stream::InitialResponsePolicy;
pub use modeled_error::HttpModeledError;
pub(crate) use protocol::body_collection_rejection;
pub use protocol::{
    collect_request_body, BodyDirective, BodyRoutedProtocol, EventStreamFraming, MetadataRoutedProtocol,
    ProtocolBuildContext, ProtocolFactory, ProtocolOrder, ProtocolRegistration, ProtocolRegistry,
    RequestBodyCollectionConfig, RequestBodyCollectionError, ServerProtocol, ServiceRequestBodyConfig,
    SharedServerProtocol,
};

pub use service::{OperationSchema, ServiceSchema};

/// The protocol and the operation selected by routing, with the operation's request-body limits,
/// stored in the request extensions.
///
/// The protocol is erased: everything after routing works through `dyn ServerProtocol`.
///
/// The operation schema is stored for consumers that are not generic over the operation:
/// middleware and [`FromParts`](crate::request::FromParts) extractors read this extension to
/// learn which operation was selected — for logging, auth, or metrics — without naming an `Op`
/// type. See the middleware example on [`ServerProtocol`].
#[derive(Clone)]
pub struct SelectedOperation {
    protocol: SharedServerProtocol,
    operation: &'static OperationSchema<'static>,
    request_body: RequestBodyCollectionConfig,
}

impl SelectedOperation {
    /// Borrows the protocol and operation selected for this request from its extensions.
    ///
    /// Returns `None` if no selection is stored, including before schema routing runs.
    pub fn get_from_request<B>(request: &http::Request<B>) -> Option<&Self> {
        request.extensions().get::<Self>()
    }

    /// Only the router creates this extension. Keeping construction crate-private means middleware
    /// cannot replace the operation's request-body limits (for example with the unlimited
    /// `RequestBodyCollectionConfig::default()`); to route through another protocol, use
    /// [`with_protocol`](Self::with_protocol), which keeps them.
    pub(crate) fn new(
        protocol: SharedServerProtocol,
        operation: &'static OperationSchema<'static>,
        request_body: RequestBodyCollectionConfig,
    ) -> Self {
        Self {
            protocol,
            operation,
            request_body,
        }
    }

    /// The same routed operation served through `protocol`. The operation and its request-body
    /// limits are kept, so middleware that swaps the protocol cannot loosen them.
    pub fn with_protocol(&self, protocol: SharedServerProtocol) -> Self {
        Self {
            protocol,
            operation: self.operation,
            request_body: self.request_body,
        }
    }

    /// The selected protocol.
    pub fn protocol(&self) -> &SharedServerProtocol {
        &self.protocol
    }

    /// The routed operation's schema, for code that cannot name the operation type.
    pub fn operation(&self) -> &'static OperationSchema<'static> {
        self.operation
    }

    /// The routed operation's request-body collection limits, resolved when the router was built.
    ///
    /// Carrying them here keeps the per-operation upgrade services free of per-operation state.
    pub fn request_body_config(&self) -> RequestBodyCollectionConfig {
        self.request_body
    }
}

impl std::fmt::Debug for SelectedOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedOperation")
            .field("protocol", &self.protocol.protocol_id())
            .field("operation", &self.operation.shape_id())
            .field("request_body", &self.request_body)
            .finish()
    }
}
