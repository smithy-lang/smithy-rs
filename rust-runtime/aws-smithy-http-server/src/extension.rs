/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Extension types.
//!
//! Extension types are types that are stored in and extracted from _both_ requests and
//! responses.
//!
//! There is only one _generic_ extension type _for requests_, [`Extension`].
//!
//! On the other hand, the server SDK uses multiple concrete extension types for responses in order
//! to store a variety of information, like the operation that was executed, the operation error
//! that got returned, or the runtime error that happened, among others. The information stored in
//! these types may be useful to [`tower::Layer`]s that post-process the response: for instance, a
//! particular metrics layer implementation might want to emit metrics about the number of times an
//! an operation got executed.
//!
//! [extensions]: https://docs.rs/http/latest/http/struct.Extensions.html

use std::{fmt, future::Future, ops::Deref, pin::Pin, task::Context, task::Poll};

use futures_util::ready;
use futures_util::TryFuture;
use thiserror::Error;
use tower::{layer::util::Stack, Layer, Service};

use crate::operation::{Operation, OperationShape};
use crate::plugin::{plugin_from_operation_name_fn, OperationNameFn, Plugin, PluginPipeline, PluginStack};

pub use crate::request::extension::{Extension, MissingExtension};

/// Extension type used to store information about Smithy operations in HTTP responses.
/// This extension type is inserted, via the [`OperationExtensionPlugin`], whenever it has been correctly determined
/// that the request should be routed to a particular operation. The operation handler might not even get invoked
/// because the request fails to deserialize into the modeled operation input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationExtension {
    absolute: &'static str,

    namespace: &'static str,
    name: &'static str,
}

/// An error occurred when parsing an absolute operation shape ID.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    #[error("# was not found - missing namespace")]
    MissingNamespace,
}

#[allow(deprecated)]
impl OperationExtension {
    /// Creates a new [`OperationExtension`] from the absolute shape ID.
    pub fn new(absolute_operation_id: &'static str) -> Result<Self, ParseError> {
        let (namespace, name) = absolute_operation_id
            .rsplit_once('#')
            .ok_or(ParseError::MissingNamespace)?;
        Ok(Self {
            absolute: absolute_operation_id,
            namespace,
            name,
        })
    }

    /// Returns the Smithy model namespace.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Returns the Smithy operation name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the absolute operation shape ID.
    pub fn absolute(&self) -> &'static str {
        self.absolute
    }
}

pin_project_lite::pin_project! {
    /// The [`Service::Future`] of [`OperationExtensionService`] - inserts an [`OperationExtension`] into the
    /// [`http::Response]`.
    pub struct OperationExtensionFuture<Fut> {
        #[pin]
        inner: Fut,
        operation_extension: Option<OperationExtension>
    }
}

impl<Fut, RespB> Future for OperationExtensionFuture<Fut>
where
    Fut: TryFuture<Ok = http::Response<RespB>>,
{
    type Output = Result<http::Response<RespB>, Fut::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let resp = ready!(this.inner.try_poll(cx));
        let ext = this
            .operation_extension
            .take()
            .expect("futures cannot be polled after completion");
        Poll::Ready(resp.map(|mut resp| {
            resp.extensions_mut().insert(ext);
            resp
        }))
    }
}

/// Inserts a [`OperationExtension`] into the extensions of the [`http::Response`].
#[derive(Debug, Clone)]
pub struct OperationExtensionService<S> {
    inner: S,
    operation_extension: OperationExtension,
}

impl<S, B, RespBody> Service<http::Request<B>> for OperationExtensionService<S>
where
    S: Service<http::Request<B>, Response = http::Response<RespBody>>,
{
    type Response = http::Response<RespBody>;
    type Error = S::Error;
    type Future = OperationExtensionFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        OperationExtensionFuture {
            inner: self.inner.call(req),
            operation_extension: Some(self.operation_extension.clone()),
        }
    }
}

/// A [`Layer`] applying the [`OperationExtensionService`] to an inner [`Service`].
#[derive(Debug, Clone)]
pub struct OperationExtensionLayer(OperationExtension);

impl<S> Layer<S> for OperationExtensionLayer {
    type Service = OperationExtensionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OperationExtensionService {
            inner,
            operation_extension: self.0.clone(),
        }
    }
}

/// A [`Plugin`] which applies [`OperationExtensionLayer`] to every operation.
pub struct OperationExtensionPlugin(OperationNameFn<fn(&'static str) -> OperationExtensionLayer>);

impl fmt::Debug for OperationExtensionPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OperationExtensionPlugin").field(&"...").finish()
    }
}

impl<P, Op, S, L> Plugin<P, Op, S, L> for OperationExtensionPlugin
where
    Op: OperationShape,
{
    type Service = S;
    type Layer = Stack<L, OperationExtensionLayer>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        <OperationNameFn<fn(&'static str) -> OperationExtensionLayer> as Plugin<P, Op, S, L>>::map(&self.0, input)
    }
}

/// An extension trait on [`PluginPipeline`] allowing the application of [`OperationExtensionPlugin`].
///
/// See [`module`](crate::extension) documentation for more info.
pub trait OperationExtensionExt<P> {
    /// Apply the [`OperationExtensionPlugin`], which inserts the [`OperationExtension`] into every [`http::Response`].
    fn insert_operation_extension(self) -> PluginPipeline<PluginStack<OperationExtensionPlugin, P>>;
}

impl<P> OperationExtensionExt<P> for PluginPipeline<P> {
    fn insert_operation_extension(self) -> PluginPipeline<PluginStack<OperationExtensionPlugin, P>> {
        let plugin = OperationExtensionPlugin(plugin_from_operation_name_fn(|name| {
            let operation_extension = OperationExtension::new(name).expect("Operation name is malformed, this should never happen. Please file an issue against https://github.com/awslabs/smithy-rs");
            OperationExtensionLayer(operation_extension)
        }));
        self.push(plugin)
    }
}

/// Extension type used to store the type of user-modeled error returned by an operation handler.
/// These are modeled errors, defined in the Smithy model.
#[derive(Debug, Clone)]
pub struct ModeledErrorExtension(&'static str);

impl ModeledErrorExtension {
    /// Creates a new `ModeledErrorExtension`.
    pub fn new(value: &'static str) -> ModeledErrorExtension {
        ModeledErrorExtension(value)
    }
}

impl Deref for ModeledErrorExtension {
    type Target = &'static str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extension type used to store the _name_ of the possible runtime errors.
/// These are _unmodeled_ errors; the operation handler was not invoked.
#[derive(Debug, Clone)]
pub struct RuntimeErrorExtension(String);

impl RuntimeErrorExtension {
    /// Creates a new `RuntimeErrorExtension`.
    pub fn new(value: String) -> RuntimeErrorExtension {
        RuntimeErrorExtension(value)
    }
}

impl Deref for RuntimeErrorExtension {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use tower::{service_fn, ServiceExt};

    use crate::{operation::OperationShapeExt, proto::rest_json_1::RestJson1};

    use super::*;

    #[test]
    fn ext_accept() {
        let value = "com.amazonaws.ebs#CompleteSnapshot";
        let ext = OperationExtension::new(value).unwrap();

        assert_eq!(ext.absolute(), value);
        assert_eq!(ext.namespace(), "com.amazonaws.ebs");
        assert_eq!(ext.name(), "CompleteSnapshot");
    }

    #[test]
    fn ext_reject() {
        let value = "CompleteSnapshot";
        assert_eq!(
            OperationExtension::new(value).unwrap_err(),
            ParseError::MissingNamespace
        )
    }

    #[tokio::test]
    async fn plugin() {
        struct DummyOp;

        impl OperationShape for DummyOp {
            const NAME: &'static str = "com.amazonaws.ebs#CompleteSnapshot";

            type Input = ();
            type Output = ();
            type Error = ();
        }

        // Apply `Plugin`.
        let operation = DummyOp::from_handler(|_| async { Ok(()) });
        let plugins = PluginPipeline::new().insert_operation_extension();
        let op = Plugin::<RestJson1, DummyOp, _, _>::map(&plugins, operation);

        // Apply `Plugin`s `Layer`.
        let layer = op.layer;
        let svc = service_fn(|_: http::Request<()>| async { Ok::<_, ()>(http::Response::new(())) });
        let svc = layer.layer(svc);

        // Check for `OperationExtension`.
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        let expected = OperationExtension::new(DummyOp::NAME).unwrap();
        let actual = response.extensions().get::<OperationExtension>().unwrap();
        assert_eq!(*actual, expected);
    }
}
