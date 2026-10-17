/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The standard routing rejection shared by every schema-mode protocol router.
//!
//! Routers classify a rejection into one of three [`RoutingErrorKind`]s; how a kind looks on
//! the wire is each protocol's decision, made in
//! [`serialize_routing_error`](crate::schema::ServerProtocol::serialize_routing_error). The
//! default there keeps the historical smithy-rs responses: every kind collapses into one of
//! two member-less error shapes — `UnknownOperationException` (`404`) and
//! `MethodNotAllowedException` (`405`) — framed by the protocol's own
//! [`serialize_error`](crate::schema::ServerProtocol::serialize_error). Diagnostic detail
//! never reaches the wire; it travels in the error's [`source`](std::error::Error::source)
//! chain for logs and tests.

use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType, StringTrait, TraitMap};

use crate::schema::HttpModeledError;

static ERROR_CLIENT_TRAITS: std::sync::LazyLock<TraitMap> = std::sync::LazyLock::new(|| {
    let mut map = TraitMap::new();
    map.insert(Box::new(StringTrait::new(shape_id!("smithy.api", "error"), "client")));
    map
});

/// The wire shape of a "no such operation" routing rejection: `404`, member-less.
pub static UNKNOWN_OPERATION: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.framework", "UnknownOperationException"),
    ShapeType::Structure,
    &[],
)
.with_traits(&ERROR_CLIENT_TRAITS);

/// The wire shape of a "method not allowed" routing rejection: `405`, member-less.
pub static METHOD_NOT_ALLOWED: Schema<'static> = Schema::new_struct(
    shape_id!("smithy.framework", "MethodNotAllowedException"),
    ShapeType::Structure,
    &[],
)
.with_traits(&ERROR_CLIENT_TRAITS);

/// The routing-semantic classification of a rejection.
///
/// Deliberately coarse: an empirical audit of Coral's responses showed no protocol puts
/// finer detail on the wire, so anything finer belongs in [`RoutingError`]'s source chain.
/// `#[non_exhaustive]` so a protocol that someday needs a new wire distinction can get a
/// kind instead of downcasting sources.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingErrorKind {
    /// The request is framed as this protocol but names no operation the service serves —
    /// or is missing what would name one (e.g. `X-Amz-Target`).
    UnknownOperation,
    /// The URI matched an operation but the HTTP method did not.
    ///
    /// Only REST routing can produce this: RPC protocols don't route on the method, so a
    /// wrong verb there is indistinguishable from an unknown operation. Coral has no `405`
    /// at all; a Coral-parity protocol maps this kind to its unknown-operation response.
    MethodNotAllowed,
    /// The request claims the protocol but violates its framing rules — forbidden headers,
    /// a malformed rpcv2 path, an invalid header value.
    MalformedRequest,
}

/// A schema-mode routing rejection: a [`RoutingErrorKind`] plus an optional diagnostic
/// source that never reaches the wire.
#[derive(Debug)]
pub struct RoutingError {
    kind: RoutingErrorKind,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl RoutingError {
    /// A rejection naming no operation the service serves.
    pub fn unknown_operation() -> Self {
        Self {
            kind: RoutingErrorKind::UnknownOperation,
            source: None,
        }
    }

    /// A rejection for a URI whose matched operations accept a different method.
    pub fn method_not_allowed() -> Self {
        Self {
            kind: RoutingErrorKind::MethodNotAllowed,
            source: None,
        }
    }

    /// A rejection for a request that violates the protocol's framing rules; `source` names
    /// the violation for logs.
    pub fn malformed(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind: RoutingErrorKind::MalformedRequest,
            source: Some(Box::new(source)),
        }
    }

    /// Attaches a diagnostic source.
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Returns the routing-semantic classification.
    pub fn kind(&self) -> RoutingErrorKind {
        self.kind
    }
}

impl std::fmt::Display for RoutingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            RoutingErrorKind::UnknownOperation => f.write_str("unknown operation"),
            RoutingErrorKind::MethodNotAllowed => f.write_str("method not allowed"),
            RoutingErrorKind::MalformedRequest => f.write_str("malformed request"),
        }
    }
}

impl std::error::Error for RoutingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|err| err as _)
    }
}

// The smithy-rs default wire form: `MethodNotAllowed` is the `405` shape, every other kind
// the `404`, exactly as routing rejections have always answered. A protocol wanting a
// different wire form overrides `serialize_routing_error` rather than these impls.
impl SerializableStruct for RoutingError {
    fn schema(&self) -> &Schema<'_> {
        match self.kind {
            RoutingErrorKind::MethodNotAllowed => &METHOD_NOT_ALLOWED,
            _ => &UNKNOWN_OPERATION,
        }
    }

    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

impl HttpModeledError for RoutingError {
    fn status_code(&self) -> u16 {
        match self.kind {
            RoutingErrorKind::MethodNotAllowed => 405,
            _ => 404,
        }
    }
}

impl From<crate::protocol::rest::router::Error> for RoutingError {
    fn from(err: crate::protocol::rest::router::Error) -> Self {
        use crate::protocol::rest::router::Error;
        match err {
            Error::NotFound => Self::unknown_operation(),
            Error::MethodNotAllowed => Self::method_not_allowed(),
        }
    }
}

impl From<crate::protocol::aws_json::router::Error> for RoutingError {
    fn from(err: crate::protocol::aws_json::router::Error) -> Self {
        use crate::protocol::aws_json::router::Error;
        match err {
            // A missing `X-Amz-Target` (or a non-root URI) is indistinguishable from an
            // unknown operation, matching Coral's identical responses for both.
            Error::NotFound | Error::NotRootUrl | Error::MissingHeader => Self::unknown_operation(),
            Error::MethodNotAllowed => Self::method_not_allowed(),
            Error::InvalidHeader(_) => Self::malformed(err),
        }
    }
}

impl From<crate::protocol::rpc_v2_cbor::router::Error> for RoutingError {
    fn from(err: crate::protocol::rpc_v2_cbor::router::Error) -> Self {
        use crate::protocol::rpc_v2_cbor::router::{Error, WireFormatError};
        match err {
            Error::NotFound => Self::unknown_operation(),
            Error::MethodNotAllowed => Self::method_not_allowed(),
            Error::ForbiddenHeaders => Self::malformed(err),
            // A missing or unrecognized `smithy-protocol` header means the request never
            // identified the protocol — Coral's rpcv2 handler falls through to the generic
            // unknown-operation response there.
            Error::InvalidWireFormatHeader(
                WireFormatError::HeaderNotFound | WireFormatError::WireFormatNotSupported(_),
            ) => Self::unknown_operation().with_source(err),
            // The header is present and rpcv2-shaped but its value is invalid: the request
            // claims the protocol and violates it.
            Error::InvalidWireFormatHeader(_) => Self::malformed(err),
        }
    }
}
