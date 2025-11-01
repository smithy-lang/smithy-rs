/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The shape of a [Smithy service] is modelled by the [`ServiceShape`] trait. Its associated types
//! [`ServiceShape::ID`], [`ServiceShape::VERSION`], [`ServiceShape::Protocol`], and [`ServiceShape::Operations`] map
//! to the services [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id), the version field, the applied
//! [protocol trait](https://smithy.io/2.0/aws/protocols/index.html) (see [`protocol`](crate::protocol) module), and the
//! operations field.
//!
//! We generate an implementation on this for every service struct (exported from the root of the generated crate).
//!
//! As stated in the [operation module documentation](crate::operation) we also generate marker structs for
//! [`OperationShape`](crate::operation::OperationShape), these are coupled to the `S: ServiceShape` via the [`ContainsOperation`] trait.
//!
//! The model
//!
//! ```smithy
//! @restJson1
//! service Shopping {
//!     version: "1.0",
//!     operations: [
//!         GetShopping,
//!         PutShopping
//!     ]
//! }
//! ```
//!
//! is identified with the implementation
//!
//! ```rust,no_run
//! # use aws_smithy_legacy_http_server::shape_id::ShapeId;
//! # use aws_smithy_legacy_http_server::service::{ServiceShape, ContainsOperation};
//! # use aws_smithy_legacy_http_server::protocol::rest_json_1::RestJson1;
//! # pub struct Shopping;
//! // For more information on these marker structs see `OperationShape`
//! struct GetShopping;
//! struct PutShopping;
//!
//! // This is a generated enumeration of all operations.
//! #[derive(PartialEq, Eq, Clone, Copy)]
//! pub enum Operation {
//!     GetShopping,
//!     PutShopping
//! }
//!
//! impl ServiceShape for Shopping {
//!     const ID: ShapeId = ShapeId::new("namespace#Shopping", "namespace", "Shopping");
//!     const VERSION: Option<&'static str> = Some("1.0");
//!     type Protocol = RestJson1;
//!     type Operations = Operation;
//! }
//!
//! impl ContainsOperation<GetShopping> for Shopping {
//!     const VALUE: Operation = Operation::GetShopping;
//! }
//!
//! impl ContainsOperation<PutShopping> for Shopping {
//!     const VALUE: Operation = Operation::PutShopping;
//! }
//! ```
//!
//! [Smithy service]: https://smithy.io/2.0/spec/service-types.html#service

use crate::shape_id::ShapeId;

/// Models the [Smithy Service shape].
///
/// [Smithy Service shape]: https://smithy.io/2.0/spec/service-types.html#service
pub trait ServiceShape {
    /// The [`ShapeId`] of the service.
    const ID: ShapeId;

    /// The version of the service.
    const VERSION: Option<&'static str>;

    /// The [Protocol] applied to this service.
    ///
    /// [Protocol]: https://smithy.io/2.0/spec/protocol-traits.html
    type Protocol;

    /// An enumeration of all operations contained in this service.
    type Operations;
}

pub trait ContainsOperation<Op>: ServiceShape {
    const VALUE: Self::Operations;
}
