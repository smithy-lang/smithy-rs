/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The plugin system allows you to build middleware with an awareness of the operation it is applied to.
//!
//! The system centers around the [`Plugin`], [`HttpMarker`], and [`ModelMarker`] traits. In
//! addition, this module provides helpers for composing and combining [`Plugin`]s.
//!
//! # HTTP plugins vs model plugins
//!
//! Plugins come in two flavors: _HTTP_ plugins and _model_ plugins. The key difference between
//! them is _when_ they run:
//!
//! - A HTTP plugin acts on the HTTP request before it is deserialized, and acts on the HTTP response
//!   after it is serialized.
//! - A model plugin acts on the modeled operation input after it is deserialized, and acts on the
//!   modeled operation output or the modeled operation error before it is serialized.
//!
//! See the relevant section in [the book], which contains an illustrative diagram.
//!
//! Both kinds of plugins implement the [`Plugin`] trait, but only HTTP plugins implement the
//! [`HttpMarker`] trait and only model plugins implement the [`ModelMarker`] trait. There is no
//! difference in how an HTTP plugin or a model plugin is applied, so both the [`HttpMarker`] trait
//! and the [`ModelMarker`] trait are _marker traits_, they carry no behavior. Their only purpose
//! is to mark a plugin, at the type system leve, as allowed to run at a certain time. A plugin can be
//! _both_ a HTTP plugin and a model plugin by implementing both traits; in this case, when the
//! plugin runs is decided by you when you register it in your application. [`IdentityPlugin`],
//! [`Scoped`], and [`LayerPlugin`] are examples of plugins that implement both traits.
//!
//! In practice, most plugins are HTTP plugins. Since HTTP plugins run before a request has been
//! correctly deserialized, HTTP plugins should be fast and lightweight. Only use model plugins if
//! you absolutely require your middleware to run after deserialization, or to act on particular
//! fields of your deserialized operation's input/output/errors.
//!
//! [the book]: https://smithy-lang.github.io/smithy-rs/design/server/anatomy.html
//!
//! # Filtered application of a HTTP [`Layer`](tower::Layer)
//!
//! ```
//! # use aws_smithy_http_server::plugin::*;
//! # use aws_smithy_http_server::scope;
//! # use aws_smithy_http_server::shape_id::ShapeId;
//! # let layer = ();
//! # #[derive(PartialEq)]
//! # enum Operation { GetPokemonSpecies }
//! # struct GetPokemonSpecies;
//! # impl GetPokemonSpecies { const ID: ShapeId = ShapeId::new("namespace#name", "namespace", "name"); };
//! // Create a `Plugin` from a HTTP `Layer`
//! let plugin = LayerPlugin(layer);
//!
//! scope! {
//!     struct OnlyGetPokemonSpecies {
//!         includes: [GetPokemonSpecies],
//!         excludes: [/* The rest of the operations go here */]
//!     }
//! }
//!
//! // Only apply the layer to operations with name "GetPokemonSpecies".
//! let filtered_plugin = Scoped::new::<OnlyGetPokemonSpecies>(&plugin);
//!
//! // The same effect can be achieved at runtime.
//! let filtered_plugin = filter_by_operation(&plugin, |operation: Operation| operation == Operation::GetPokemonSpecies);
//! ```
//!
//! # Construct a [`Plugin`] from a closure that takes as input the operation name
//!
//! ```rust
//! # use aws_smithy_http_server::{service::*, operation::OperationShape, plugin::Plugin, shape_id::ShapeId};
//! # pub enum Operation { CheckHealth, GetPokemonSpecies }
//! # impl Operation { fn shape_id(&self) -> ShapeId { ShapeId::new("", "", "") }}
//! # pub struct CheckHealth;
//! # pub struct GetPokemonSpecies;
//! # pub struct PokemonService;
//! # impl ServiceShape for PokemonService {
//! #   const ID: ShapeId = ShapeId::new("", "", "");
//! #   const VERSION: Option<&'static str> = None;
//! #   type Protocol = ();
//! #   type Operations = Operation;
//! # }
//! # impl OperationShape for CheckHealth { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
//! # impl OperationShape for GetPokemonSpecies { const ID: ShapeId = ShapeId::new("", "", ""); type Input = (); type Output = (); type Error = (); }
//! # impl ContainsOperation<CheckHealth> for PokemonService { const VALUE: Operation = Operation::CheckHealth; }
//! # impl ContainsOperation<GetPokemonSpecies> for PokemonService { const VALUE: Operation = Operation::GetPokemonSpecies; }
//! use aws_smithy_http_server::plugin::plugin_from_operation_fn;
//! use tower::layer::layer_fn;
//!
//! struct FooService<S> {
//!     info: String,
//!     inner: S
//! }
//!
//! fn map<S>(op: Operation, inner: S) -> FooService<S> {
//!     match op {
//!         Operation::CheckHealth => FooService { info: op.shape_id().name().to_string(), inner },
//!         Operation::GetPokemonSpecies => FooService { info: "bar".to_string(), inner },
//!         _ => todo!()
//!     }
//! }
//!
//! // This plugin applies the `FooService` middleware around every operation.
//! let plugin = plugin_from_operation_fn(map);
//! # let _ = Plugin::<PokemonService, CheckHealth, ()>::apply(&plugin, ());
//! # let _ = Plugin::<PokemonService, GetPokemonSpecies, ()>::apply(&plugin, ());
//! ```
//!
//! # Combine [`Plugin`]s
//!
//! ```no_run
//! # use aws_smithy_http_server::plugin::*;
//! # struct Foo;
//! # impl HttpMarker for Foo { }
//! # let a = Foo; let b = Foo;
//! // Combine `Plugin`s `a` and `b`. Both need to implement `HttpMarker`.
//! let plugin = HttpPlugins::new()
//!     .push(a)
//!     .push(b);
//! ```
//!
//! As noted in the [`HttpPlugins`] documentation, the plugins' runtime logic is executed in registration order,
//! meaning that `a` is run _before_ `b` in the example above.
//!
//! Similarly, you can use [`ModelPlugins`] to combine model plugins.
//!
//! # Example implementation of a [`Plugin`]
//!
//! The following is an example implementation of a [`Plugin`] that prints out the service's name
//! and the name of the operation that was hit every time it runs. Since it doesn't act on the HTTP
//! request nor the modeled operation input/output/errors, this plugin can be both an HTTP plugin
//! and a model plugin. In practice, however, you'd only want to register it once, as either an
//! HTTP plugin or a model plugin.
//!
//! ```no_run
//! use aws_smithy_http_server::{
//!     operation::OperationShape,
//!     service::ServiceShape,
//!     plugin::{Plugin, HttpMarker, HttpPlugins, ModelMarker},
//!     shape_id::ShapeId,
//! };
//! # use tower::{layer::util::Stack, Layer, Service};
//! # use std::task::{Context, Poll};
//!
//! /// A [`Service`] that adds a print log.
//! #[derive(Clone, Debug)]
//! pub struct PrintService<S> {
//!     inner: S,
//!     service_id: ShapeId,
//!     operation_id: ShapeId
//! }
//!
//! impl<R, S> Service<R> for PrintService<S>
//! where
//!     S: Service<R>,
//! {
//!     type Response = S::Response;
//!     type Error = S::Error;
//!     type Future = S::Future;
//!
//!     fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//!         self.inner.poll_ready(cx)
//!     }
//!
//!     fn call(&mut self, req: R) -> Self::Future {
//!         println!("Hi {} in {}", self.operation_id.absolute(), self.service_id.absolute());
//!         self.inner.call(req)
//!     }
//! }
//!
//! /// A [`Plugin`] for a service builder to add a [`PrintLayer`] over operations.
//! #[derive(Debug)]
//! pub struct PrintPlugin;
//!
//! impl<Ser, Op, T> Plugin<Ser, Op, T> for PrintPlugin
//! where
//!     Ser: ServiceShape,
//!     Op: OperationShape,
//! {
//!     type Output = PrintService<T>;
//!
//!     fn apply(&self, inner: T) -> Self::Output {
//!         PrintService {
//!             inner,
//!             service_id: Op::ID,
//!             operation_id: Ser::ID,
//!         }
//!     }
//! }
//!
//! // This plugin could be registered as an HTTP plugin and a model plugin, so we implement both
//! // marker traits.
//!
//! impl HttpMarker for PrintPlugin { }
//! impl ModelMarker for PrintPlugin { }
//! ```

mod closure;
pub(crate) mod either;
mod filter;
mod http_plugins;
mod identity;
mod layer;
mod model_plugins;
#[doc(hidden)]
pub mod scoped;
mod stack;

pub use closure::{plugin_from_operation_fn, OperationFn};
pub use either::Either;
pub use filter::{filter_by_operation, FilterByOperation};
pub use http_plugins::HttpPlugins;
pub use identity::IdentityPlugin;
pub use layer::{LayerPlugin, PluginLayer};
pub use model_plugins::ModelPlugins;
pub use scoped::Scoped;
pub use stack::PluginStack;

/// A mapping from one [`Service`](tower::Service) to another. This should be viewed as a
/// [`Layer`](tower::Layer) parameterized by the protocol and operation.
///
/// The generics `Ser` and `Op` allow the behavior to be parameterized by the [Smithy service] and
/// [operation] it's applied to.
///
/// See [module](crate::plugin) documentation for more information.
///
/// [Smithy service]: https://smithy.io/2.0/spec/service-types.html#service
/// [operation]: https://smithy.io/2.0/spec/service-types.html#operation
pub trait Plugin<Ser, Op, T> {
    /// The type of the new [`Service`](tower::Service).
    type Output;

    /// Maps a [`Service`](tower::Service) to another.
    fn apply(&self, input: T) -> Self::Output;
}

impl<Ser, Op, T, Pl> Plugin<Ser, Op, T> for &Pl
where
    Pl: Plugin<Ser, Op, T>,
{
    type Output = Pl::Output;

    fn apply(&self, inner: T) -> Self::Output {
        <Pl as Plugin<Ser, Op, T>>::apply(self, inner)
    }
}

/// A HTTP plugin is a plugin that acts on the HTTP request before it is deserialized, and acts on
/// the HTTP response after it is serialized.
///
/// This trait is a _marker_ trait to indicate that a plugin can be registered as an HTTP plugin.
///
/// Compare with [`ModelMarker`] in the [module](crate::plugin) documentation, which contains an
/// example implementation too.
pub trait HttpMarker {}
impl<Pl> HttpMarker for &Pl where Pl: HttpMarker {}

/// A model plugin is a plugin that acts on the modeled operation input after it is deserialized,
/// and acts on the modeled operation output or the modeled operation error before it is
/// serialized.
///
/// This trait is a _marker_ trait to indicate that a plugin can be registered as a model plugin.
///
/// Compare with [`HttpMarker`] in the [module](crate::plugin) documentation.
///
/// # Example implementation of a model plugin
///
/// Model plugins are most useful when you really need to rely on the actual shape of your modeled
/// operation input, operation output, and/or operation errors. For this reason, most (but not all)
/// model plugins are _operation-specific_: somewhere in the type signature of their definition,
/// they'll rely on a particular operation shape's types. It is therefore important that you scope
/// application of model plugins to the operations they are meant to work on, via
/// [`Scoped`] or [`filter_by_operation`].
///
/// Below is an example implementation of a model plugin that can only be applied to the
/// `CheckHealth` operation: note how in the `Service` trait implementation, we require access to
/// the operation's input, where we log the `health_info` field.
///
/// ```no_run
/// use std::marker::PhantomData;
///
/// use aws_smithy_http_server::{operation::OperationShape, plugin::{ModelMarker, Plugin}};
/// use tower::Service;
/// # pub struct SimpleService;
/// # pub struct CheckHealth;
/// # pub struct CheckHealthInput {
/// #     health_info: (),
/// # }
/// # pub struct CheckHealthOutput;
/// # impl aws_smithy_http_server::operation::OperationShape for CheckHealth {
/// #     const ID: aws_smithy_http_server::shape_id::ShapeId = aws_smithy_http_server::shape_id::ShapeId::new(
/// #         "com.amazonaws.simple#CheckHealth",
/// #         "com.amazonaws.simple",
/// #         "CheckHealth",
/// #     );
/// #     type Input = CheckHealthInput;
/// #     type Output = CheckHealthOutput;
/// #     type Error = std::convert::Infallible;
/// # }
///
/// /// A model plugin that can only be applied to the `CheckHealth` operation.
/// pub struct CheckHealthPlugin<Exts> {
///     pub _exts: PhantomData<Exts>,
/// }
///
/// impl<Exts> CheckHealthPlugin<Exts> {
///     pub fn new() -> Self {
///         Self { _exts: PhantomData }
///     }
/// }
///
/// impl<T, Exts> Plugin<SimpleService, CheckHealth, T> for CheckHealthPlugin<Exts> {
///     type Output = CheckHealthService<T, Exts>;
///
///     fn apply(&self, input: T) -> Self::Output {
///         CheckHealthService {
///             inner: input,
///             _exts: PhantomData,
///         }
///     }
/// }
///
/// impl<Exts> ModelMarker for CheckHealthPlugin<Exts> { }
///
/// #[derive(Clone)]
/// pub struct CheckHealthService<S, Exts> {
///     inner: S,
///     _exts: PhantomData<Exts>,
/// }
///
/// impl<S, Exts> Service<(<CheckHealth as OperationShape>::Input, Exts)> for CheckHealthService<S, Exts>
/// where
///     S: Service<(<CheckHealth as OperationShape>::Input, Exts)>,
/// {
///     type Response = S::Response;
///     type Error = S::Error;
///     type Future = S::Future;
///
///     fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
///         self.inner.poll_ready(cx)
///     }
///
///     fn call(&mut self, req: (<CheckHealth as OperationShape>::Input, Exts)) -> Self::Future {
///         let (input, _exts) = &req;
///
///         // We have access to `CheckHealth`'s modeled operation input!
///         dbg!(&input.health_info);
///
///         self.inner.call(req)
///     }
/// }
///
/// // In `main.rs` or wherever we register plugins, we have to make sure we only apply this plugin
/// // to the the only operation it can be applied to, the `CheckHealth` operation. If we apply the
/// // plugin to other operations, we will get a compilation error.
///
/// use aws_smithy_http_server::plugin::Scoped;
/// use aws_smithy_http_server::scope;
///
/// pub fn main() {
///     scope! {
///         struct OnlyCheckHealth {
///             includes: [CheckHealth],
///             excludes: [/* The rest of the operations go here */]
///         }
///     }
///
///     let model_plugin = CheckHealthPlugin::new();
///     # _foo(&model_plugin);
///
///     // Scope the plugin to the `CheckHealth` operation.
///     let scoped_plugin = Scoped::new::<OnlyCheckHealth>(model_plugin);
///     # fn _foo(model_plugin: &CheckHealthPlugin<()>) {}
/// }
/// ```
///
/// If you are a service owner and don't care about giving a name to the model plugin, you can
/// simplify this down to:
///
/// ```no_run
/// use std::marker::PhantomData;
///
/// use aws_smithy_http_server::operation::OperationShape;
/// use tower::Service;
/// # pub struct SimpleService;
/// # pub struct CheckHealth;
/// # pub struct CheckHealthInput {
/// #     health_info: (),
/// # }
/// # pub struct CheckHealthOutput;
/// # impl aws_smithy_http_server::operation::OperationShape for CheckHealth {
/// #     const ID: aws_smithy_http_server::shape_id::ShapeId = aws_smithy_http_server::shape_id::ShapeId::new(
/// #         "com.amazonaws.simple#CheckHealth",
/// #         "com.amazonaws.simple",
/// #         "CheckHealth",
/// #     );
/// #     type Input = CheckHealthInput;
/// #     type Output = CheckHealthOutput;
/// #     type Error = std::convert::Infallible;
/// # }
///
/// #[derive(Clone)]
/// pub struct CheckHealthService<S, Exts> {
///     inner: S,
///     _exts: PhantomData<Exts>,
/// }
///
/// impl<S, Exts> Service<(<CheckHealth as OperationShape>::Input, Exts)> for CheckHealthService<S, Exts>
/// where
///     S: Service<(<CheckHealth as OperationShape>::Input, Exts)>,
/// {
///     type Response = S::Response;
///     type Error = S::Error;
///     type Future = S::Future;
///
///     fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
///         self.inner.poll_ready(cx)
///     }
///
///     fn call(&mut self, req: (<CheckHealth as OperationShape>::Input, Exts)) -> Self::Future {
///         let (input, _exts) = &req;
///
///         // We have access to `CheckHealth`'s modeled operation input!
///         dbg!(&input.health_info);
///
///         self.inner.call(req)
///     }
/// }
///
/// // In `main.rs`:
///
/// use aws_smithy_http_server::plugin::LayerPlugin;
/// use aws_smithy_http_server::plugin::Scoped;
/// use aws_smithy_http_server::scope;
///
/// fn new_check_health_service<S, Ext>(inner: S) -> CheckHealthService<S, Ext> {
///     CheckHealthService {
///         inner,
///         _exts: PhantomData,
///     }
/// }
///
/// pub fn main() {
///     scope! {
///         struct OnlyCheckHealth {
///             includes: [CheckHealth],
///             excludes: [/* The rest of the operations go here */]
///         }
///     }
///
///     # fn new_check_health_service(inner: ()) -> CheckHealthService<(), ()> {
///     #     CheckHealthService {
///     #         inner,
///     #         _exts: PhantomData,
///     #     }
///     # }
///     let layer = tower::layer::layer_fn(new_check_health_service);
///     let model_plugin = LayerPlugin(layer);
///
///     // Scope the plugin to the `CheckHealth` operation.
///     let scoped_plugin = Scoped::new::<OnlyCheckHealth>(model_plugin);
/// }
/// ```
pub trait ModelMarker {}
impl<Pl> ModelMarker for &Pl where Pl: ModelMarker {}
