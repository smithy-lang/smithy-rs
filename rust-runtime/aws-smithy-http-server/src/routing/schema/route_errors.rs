/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-backed modeled errors for routing rejections.
//!
//! The routers' error enums keep their diagnostic variants; on the wire every cause collapses
//! into one of two member-less error shapes, exactly as routing rejections always have:
//! `UnknownOperationException` (`404`) and `MethodNotAllowedException` (`405`). The active
//! variant's schema names the discriminator and the status, and the rejecting protocol's
//! `serialize_error` does the framing.

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

/// Implements the modeled-error traits for a router error enum: `$method_not_allowed` maps to
/// the `405` shape, every other variant to the `404`, keeping the enum's Rust-side detail while
/// collapsing on the wire as the legacy responses did.
macro_rules! modeled_route_error {
    ($error:ty, $method_not_allowed:pat) => {
        impl SerializableStruct for $error {
            fn schema(&self) -> &Schema<'_> {
                match self {
                    $method_not_allowed => &METHOD_NOT_ALLOWED,
                    _ => &UNKNOWN_OPERATION,
                }
            }

            fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                Ok(())
            }
        }

        impl HttpModeledError for $error {
            fn status_code(&self) -> u16 {
                match self {
                    $method_not_allowed => 405,
                    _ => 404,
                }
            }
        }
    };
}

modeled_route_error!(
    crate::protocol::rest::router::Error,
    Self::MethodNotAllowed
);
modeled_route_error!(
    crate::protocol::aws_json::router::Error,
    Self::MethodNotAllowed
);
modeled_route_error!(
    crate::protocol::rpc_v2_cbor::router::Error,
    Self::MethodNotAllowed
);
