/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! # Operations
//!
//! The shape of a [Smithy operation] is modelled by the [`OperationShape`] trait, it's associated types
//! [`OperationShape::Input`], [`OperationShape::Output`], and [`OperationShape::Error`] map to the structures
//! representing the Smithy inputs, outputs, and errors respectively. When an operation error is not specified
//! [`OperationShape::Error`] is [`Infallible`](std::convert::Infallible).
//!
//! A ZST for each Smithy operation is generated and [`OperationShape`] should be implemented on it, this will be used
//! as a helper which provides static methods and parameterizes other traits.
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
//! ```rust
//! # use aws_smithy_http_server::operation::OperationShape;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! pub struct GetShopping;
//!
//! impl OperationShape for GetShopping {
//!     const NAME: &'static str = "GetShopping";
//!
//!     type Input = CartIdentifier;
//!     type Output = ShoppingCart;
//!     type Error = GetShoppingError;
//! }
//! ```
//!
//! The behavior of a Smithy operation is encoded by an [`Operation`]. The [`OperationShape`] ZSTs can be used to
//! construct specific operations using [`OperationShapeExt::from_handler`] and [`OperationShapeExt::from_service`].
//! The [from_handler](OperationShapeExt::from_handler) constructor takes a [`Handler`] whereas the
//! [from_service](OperationShapeExt::from_service) takes a [`OperationService`]. Both traits serve a similar purpose -
//! they provide a common interface over a class of structures.
//!
//! ## [`Handler`]
//!
//! The [`Handler`] trait is implemented by all closures which accept [`OperationShape::Input`] as their first
//! argument, the remaining arguments implement [`FromParts`](crate::request::FromParts), and return either
//! [`OperationShape::Output`] when [`OperationShape::Error`] is [`Infallible`](std::convert::Infallible) or
//! [`Result`]<[`OperationShape::Output`],[`OperationShape::Error`]>. The following are examples of closures which
//! implement [`Handler`]:
//!
//! ```rust
//! # use aws_smithy_http_server::Extension;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! # pub struct Context;
//! # pub struct ExtraContext;
//! // Simple handler where no error is given.
//! async fn handler_a(input: CartIdentifier) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // Handler with an extension where no error is given.
//! async fn handler_b(input: CartIdentifier, ext: Extension<Context>) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // More than one extension can be provided.
//! async fn handler_c(input: CartIdentifier, ext_1: Extension<Context>, ext_2: Extension<ExtraContext>) -> ShoppingCart {
//!     todo!()
//! }
//!
//! // When an error is given we must return a `Result`.
//! async fn handler_d(input: CartIdentifier, ext: Extension<Context>) -> Result<ShoppingCart, GetShoppingError> {
//!     todo!()
//! }
//! ```
//!
//! ## [`OperationService`]
//!
//! Similarly, the [`OperationService`] trait is implemented by all `Service<(Op::Input, ...)>` with
//! `Response = Op::Output`, and `Error = OperationError<Op::Output, PollError>`.
//!
//! We use [`OperationError`], with a `PollError` not constrained by the model, to allow the user to provide a custom
//! [`Service::poll_ready`](tower::Service::poll_ready) implementation.
//!
//! The following are examples of [`Service`](tower::Service)s which implement [`OperationService`]:
//!
//! - `Service<CartIdentifier, Response = ShoppingCart, Error = Infallible>`.
//! - `Service<(CartIdentifier, Extension<Context>), Response = ShoppingCart, Error = GetShoppingCartError>`.
//! - `Service<(CartIdentifier, Extension<Context>, Extension<ExtraContext>), Response = ShoppingCart, Error = GetShoppingCartError)`.
//!
//! Notice the parallels between [`OperationService`] and [`Handler`].
//!
//! ## Constructing an [`Operation`]
//!
//! The following is an example of using both construction approaches:
//!
//! ```rust
//! # use std::task::{Poll, Context};
//! # use aws_smithy_http_server::operation::*;
//! # use tower::Service;
//! # pub struct CartIdentifier;
//! # pub struct ShoppingCart;
//! # pub enum GetShoppingError {}
//! # pub struct GetShopping;
//! # impl OperationShape for GetShopping {
//! #    const NAME: &'static str = "GetShopping";
//! #
//! #    type Input = CartIdentifier;
//! #    type Output = ShoppingCart;
//! #    type Error = GetShoppingError;
//! # }
//! # type OpFuture = std::future::Ready<Result<ShoppingCart, OperationError<GetShoppingError, PollError>>>;
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
//! pub struct PollError;
//!
//! pub struct OpService;
//!
//! impl Service<(CartIdentifier, ())> for OpService {
//!     type Response = ShoppingCart;
//!     type Error = OperationError<GetShoppingError, PollError>;
//!     type Future = OpFuture;
//!
//!     fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
//!         // NOTE: This MUST NOT return `Err(OperationError::Model(_))`.
//!         todo!()
//!     }
//!
//!     fn call(&mut self, request: (CartIdentifier, ())) -> Self::Future {
//!         // NOTE: This MUST NOT return `Err(OperationError::Poll(_))`.
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
//! Both [`Handler`] and [`OperationService`] accept and return Smithy model structures. After an [`Operation`] is
//! constructed they are converted to a normal form
//! `Service<(Op::Input, Exts), Response = Op::Output, Error = OperationError<Op::Error, PollError>>`. The
//! [`UpgradeLayer`] acts upon such services by converting them to
//! `Service<http::Request, Response = http::Response, Error = PollError>`.
//!
//! Note that the `PollError` is still exposed, for two reasons:
//!
//! - Smithy is agnostic to `PollError` and therefore we have no prescribed way to serialize it to a [`http::Response`]
//! , unlike the operation errors.
//! - The intention of `PollError` is to signal that the underlying service is no longer able to take requests, so
//! should be discarded. See [`Service::poll_ready`](tower::Service::poll_ready).
//!
//! The [`UpgradeLayer`] and it's [`Layer::Service`] [`Upgrade`] are both parameterized by a protocol. This allows
//! for upgrading to `Service<http::Request, Response = http::Response, Error = PollError>` to be protocol dependent.
//!
//! The [`Operation::upgrade`] will apply [`UpgradeLayer`] to `S` then apply the [`Layer`] `L`. The service builder
//! provided to the user will perform this composition on `build`.
//!
//! [Smithy operation]: https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation

mod handler;
mod operation_service;
mod shape;
mod upgrade;

use tower::{
    layer::util::{Identity, Stack},
    Layer,
};

pub use handler::*;
pub use operation_service::*;
pub use shape::*;
pub use upgrade::*;

/// A Smithy operation, represented by a [`Service`](tower::Service) `S` and a [`Layer`] `L`.
///
/// The `L` is held and applied lazily during [`Operation::upgrade`].
pub struct Operation<S, L = Identity> {
    inner: S,
    layer: L,
}

type StackedUpgradeService<P, Op, E, B, L, S> = <Stack<UpgradeLayer<P, Op, E, B>, L> as Layer<S>>::Service;

impl<S, L> Operation<S, L> {
    /// Takes the [`Operation`], containing the inner [`Service`](tower::Service) `S`, the HTTP [`Layer`] `L` and
    /// composes them together using [`UpgradeLayer`] for a specific protocol and [`OperationShape`].
    ///
    /// The composition is made explicit in the method constraints and return type.
    pub fn upgrade<P, Op, E, B>(self) -> StackedUpgradeService<P, Op, E, B, L, S>
    where
        UpgradeLayer<P, Op, E, B>: Layer<S>,
        L: Layer<<UpgradeLayer<P, Op, E, B> as Layer<S>>::Service>,
    {
        let Self { inner, layer } = self;
        let layer = Stack::new(UpgradeLayer::new(), layer);
        layer.layer(inner)
    }
}

impl<Op, S, PollError> Operation<Normalize<Op, S, PollError>> {
    /// Creates an [`Operation`] from a [`Service`](tower::Service).
    pub fn from_service<Exts>(inner: S) -> Self
    where
        Op: OperationShape,
        S: OperationService<Op, Exts, PollError>,
    {
        Self {
            inner: inner.into_unflatten(),
            layer: Identity::new(),
        }
    }
}

impl<Op, H> Operation<IntoService<Op, H>> {
    /// Creates an [`Operation`] from a [`Handler`].
    pub fn from_handler<Exts>(handler: H) -> Self
    where
        Op: OperationShape,
        H: Handler<Op, Exts>,
    {
        Self {
            inner: handler.into_service(),
            layer: Identity::new(),
        }
    }
}

impl<S, L> Operation<S, L> {
    /// Applies a [`Layer`] to the operation _after_ it has been upgraded via [`Operation::upgrade`].
    pub fn layer<NewL>(self, layer: NewL) -> Operation<S, Stack<L, NewL>> {
        Operation {
            inner: self.inner,
            layer: Stack::new(self.layer, layer),
        }
    }
}

/// A marker struct indicating an [`Operation`] has not been set in a builder.
pub struct OperationNotSet;

/// The operation [`Service`](tower::Service) has two classes of failure modes - the failure models specified by
/// the Smithy model and failures to [`Service::poll_ready`](tower::Service::poll_ready).
pub enum OperationError<ModelError, PollError> {
    /// An error modelled by the Smithy model occurred.
    Model(ModelError),
    /// A [`Service::poll_ready`](tower::Service::poll_ready) failure occurred.
    Poll(PollError),
}
