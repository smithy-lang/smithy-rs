/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The shape of a [Smithy operation] is modelled by the [`OperationShape`] trait. Its associated types
//! [`OperationShape::Input`], [`OperationShape::Output`], and [`OperationShape::Error`] map to the structures
//! representing the Smithy inputs, outputs, and errors respectively. When an operation error is not specified
//! [`OperationShape::Error`] is [`Infallible`](std::convert::Infallible).
//!
//! We generate a marker struct for each Smithy operation and implement [`OperationShape`] on them. This
//! is used as a helper - providing static methods and parameterizing other traits.
//!
//! The model
//!
//! ```smithy
//! operation GetShopping {
//!    input: CartIdentifier,
//!    output: ShoppingCart,
//!    errors: [...]
//! }
//! ```
//!
//! is identified with the implementation
//!
//! ```rust,no_run
//! # use aws_smithy_legacy_http_server::shape_id::ShapeId;
//! # use aws_smithy_legacy_http_server::operation::OperationShape;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! pub struct GetShopping;
//!
//! impl OperationShape for GetShopping {
//!     const ID: ShapeId = ShapeId::new("namespace#GetShopping", "namespace", "GetShopping");
//!
//!     type Input = CartIdentifier;
//!     type Output = ShoppingCart;
//!     type Error = GetShoppingError;
//! }
//! ```
//!
//! The behavior of a Smithy operation is encoded by a [`Service`](tower::Service). The [`OperationShape`] types can
//! be used to construct specific operations using [`OperationShapeExt::from_handler`] and
//! [`OperationShapeExt::from_service`] methods. The [from_handler](OperationShapeExt::from_handler) constructor takes
//! a [`Handler`] whereas the [from_service](OperationShapeExt::from_service) takes a [`OperationService`]. Both traits
//! serve a similar purpose - they provide a common interface over a class of structures.
//!
//! ## [`Handler`]
//!
//! The [`Handler`] trait is implemented by all async functions which accept [`OperationShape::Input`] as their first
//! argument, the remaining arguments implement [`FromParts`](crate::request::FromParts), and return either
//! [`OperationShape::Output`] when [`OperationShape::Error`] is [`Infallible`](std::convert::Infallible) or
//! [`Result`]<[`OperationShape::Output`],[`OperationShape::Error`]>. The following are examples of async functions which
//! implement [`Handler`]:
//!
//! ```rust,no_run
//! # use aws_smithy_legacy_http_server::Extension;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! # pub struct Context;
//! # pub struct ExtraContext;
//! // Simple handler where no error is modelled.
//! async fn handler_a(input: CartIdentifier) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // Handler with an extension where no error is modelled.
//! async fn handler_b(input: CartIdentifier, ext: Extension<Context>) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // More than one extension can be provided.
//! async fn handler_c(input: CartIdentifier, ext_1: Extension<Context>, ext_2: Extension<ExtraContext>) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // When an error is modelled we must return a `Result`.
//! async fn handler_d(input: CartIdentifier, ext: Extension<Context>) -> Result<ShoppingCart, GetShoppingError> {
//!     todo!()
//! }
//! ```
//!
//! ## [`OperationService`]
//!
//! Similarly, the [`OperationService`] trait is implemented by all `Service<(Op::Input, ...)>` with
//! `Response = Op::Output`, and `Error = Op::Error`.
//!
//! The following are examples of [`Service`](tower::Service)s which implement [`OperationService`]:
//!
//! - `Service<CartIdentifier, Response = ShoppingCart, Error = Infallible>`.
//! - `Service<(CartIdentifier, Extension<Context>), Response = ShoppingCart, Error = GetShoppingCartError>`.
//! - `Service<(CartIdentifier, Extension<Context>, Extension<ExtraContext>), Response = ShoppingCart, Error = GetShoppingCartError)`.
//!
//! Notice the parallels between [`OperationService`] and [`Handler`].
//!
//! ## Constructing an Operation
//!
//! The following is an example of using both construction approaches:
//!
//! ```rust,no_run
//! # use std::task::{Poll, Context};
//! # use aws_smithy_legacy_http_server::operation::*;
//! # use tower::Service;
//! # use aws_smithy_legacy_http_server::shape_id::ShapeId;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! # pub struct GetShopping;
//! # impl OperationShape for GetShopping {
//! #    const ID: ShapeId = ShapeId::new("namespace#GetShopping", "namespace", "GetShopping");
//! #
//! #    type Input = CartIdentifier;
//! #    type Output = ShoppingCart;
//! #    type Error = GetShoppingError;
//! # }
//! # type OpFuture = std::future::Ready<Result<ShoppingCart, GetShoppingError>>;
//! // Construction of an `Operation` from a `Handler`.
//!
//! async fn op_handler(input: CartIdentifier) -> Result<ShoppingCart, GetShoppingError> {
//!     todo!()
//! }
//!
//! let operation = GetShopping::from_handler(op_handler);
//!
//! // Construction of an `Operation` from a `Service`.
//!
//! pub struct OpService;
//!
//! impl Service<CartIdentifier> for OpService {
//!     type Response = ShoppingCart;
//!     type Error = GetShoppingError;
//!     type Future = OpFuture;
//!
//!     fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
//!         todo!()
//!     }
//!
//!     fn call(&mut self, request: CartIdentifier) -> Self::Future {
//!         todo!()
//!     }
//! }
//!
//! let operation = GetShopping::from_service(OpService);
//!
//! ```
//!
//! ## Upgrading Smithy services to HTTP services
//!
//! Both [`Handler`] and [`OperationService`] accept and return Smithy model structures. They are converted to a
//! canonical form `Service<(Op::Input, Exts), Response = Op::Output, Error = Op::Error>`. The
//! [`UpgradePlugin`] acts upon such services by converting them to
//! `Service<http::Request, Response = http::Response, Error = Infallible>` by applying serialization/deserialization
//! and validation specified by the Smithy contract.
//!
//!
//! The [`UpgradePlugin`], being a [`Plugin`](crate::plugin::Plugin), is parameterized by a protocol. This allows for
//! upgrading to `Service<http::Request, Response = http::Response, Error = Infallible>` to be protocol dependent.
//!
//! [Smithy operation]: https://smithy.io/2.0/spec/service-types.html#operation

mod handler;
mod operation_service;
mod shape;
mod upgrade;

pub use handler::*;
pub use operation_service::*;
pub use shape::*;
pub use upgrade::*;
