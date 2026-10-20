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
pub mod traits;

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

/// A read-only view of the protocol and operation selected by routing, with the operation's
/// request-body limits.
///
/// The protocol is erased: everything after routing works through `dyn ServerProtocol`.
///
/// The operation schema is stored for consumers that are not generic over the operation:
/// middleware and [`FromParts`](crate::request::FromParts) extractors use
/// [`get_from_request`](Self::get_from_request) or [`get_from_extensions`](Self::get_from_extensions) to
/// learn which operation was selected — for logging, auth, or metrics — without naming an `Op`
/// type. See the middleware example on [`ServerProtocol`].
///
/// Routing stores this view in a private extension carrier. Inserting or removing
/// `SelectedOperation` directly in request extensions does not change the routed selection.
/// Middleware must preserve request extensions: clearing or replacing all extensions removes
/// the carrier, causing the upgrade or missing-handler fallback to return an empty `500`
/// without polling the body or invoking the handler. Routing may already have collected the body
/// before middleware runs.
#[derive(Clone)]
pub struct SelectedOperation {
    protocol: SharedServerProtocol,
    operation: &'static OperationSchema<'static>,
    request_body: RequestBodyCollectionConfig,
}

#[derive(Clone)]
struct SelectedOperationExtension(SelectedOperation);

impl SelectedOperation {
    /// Borrows the protocol and operation selected for this request from its extensions.
    ///
    /// Returns `None` if no selection is stored, including before schema routing runs.
    pub fn get_from_request<B>(request: &http::Request<B>) -> Option<&Self> {
        Self::get_from_extensions(request.extensions())
    }

    /// Borrows the routed selection from request extensions, including in a
    /// [`FromParts`](crate::request::FromParts) extractor.
    ///
    /// Returns `None` before routing or if middleware discarded the request extensions.
    pub fn get_from_extensions(extensions: &http::Extensions) -> Option<&Self> {
        extensions.get::<SelectedOperationExtension>().map(|stored| &stored.0)
    }

    pub(crate) fn get_from_runtime_request<B>(request: &aws_smithy_runtime_api::http::Request<B>) -> Option<&Self> {
        request
            .extension::<SelectedOperationExtension>()
            .map(|stored| &stored.0)
    }

    pub(crate) fn insert_into(self, extensions: &mut http::Extensions) {
        extensions.insert(SelectedOperationExtension(self));
    }

    /// Only the router creates the selection; middleware cannot replace its operation,
    /// protocol, or request-body limits.
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
