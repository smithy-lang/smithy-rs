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
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};

use futures_util::ready;
use futures_util::TryFuture;
use thiserror::Error;
use tower::{layer::util::Stack, Layer, Service};

use crate::operation::{Operation, OperationShape};
use crate::plugin::{plugin_from_operation_name_fn, OperationNameFn, Plugin, PluginPipeline, PluginStack};
use crate::shape_id::ShapeId;

pub use crate::request::extension::{Extension, MissingExtension};

/// Extension type used to store information about Smithy operations in HTTP responses.
/// This extension type is inserted, via the [`OperationIdPlugin`], whenever it has been correctly determined
/// that the request should be routed to a particular operation. The operation handler might not even get invoked
/// because the request fails to deserialize into the modeled operation input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationId(pub ShapeId);

/// An error occurred when parsing an absolute operation shape ID.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    #[error("# was not found - missing namespace")]
    MissingNamespace,
}

#[allow(deprecated)]
impl OperationId {
    /// Returns the Smithy model namespace.
    pub fn namespace(&self) -> &'static str {
        self.0.namespace()
    }

    /// Returns the Smithy operation name.
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    /// Returns the absolute operation shape ID.
    pub fn absolute(&self) -> &'static str {
        self.0.absolute()
    }
}

impl Display for OperationId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.absolute())
    }
}

impl Hash for OperationId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.absolute().hash(state)
    }
}

pin_project_lite::pin_project! {
    /// The [`Service::Future`] of [`OperationIdService`] - inserts an [`OperationId`] into the
    /// [`http::Response]`.
    pub struct OperationIdFuture<Fut> {
        #[pin]
        inner: Fut,
        operation_extension: Option<OperationId>
    }
}

impl<Fut, RespB> Future for OperationIdFuture<Fut>
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

/// Inserts a [`OperationId`] into the extensions of the [`http::Response`].
#[derive(Debug, Clone)]
pub struct OperationIdService<S> {
    inner: S,
    operation_extension: OperationId,
}

impl<S, B, RespBody> Service<http::Request<B>> for OperationIdService<S>
where
    S: Service<http::Request<B>, Response = http::Response<RespBody>>,
{
    type Response = http::Response<RespBody>;
    type Error = S::Error;
    type Future = OperationIdFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        OperationIdFuture {
            inner: self.inner.call(req),
            operation_extension: Some(self.operation_extension.clone()),
        }
    }
}

/// A [`Layer`] applying the [`OperationIdService`] to an inner [`Service`].
#[derive(Debug, Clone)]
pub struct OperationIdLayer(OperationId);

impl<S> Layer<S> for OperationIdLayer {
    type Service = OperationIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OperationIdService {
            inner,
            operation_extension: self.0.clone(),
        }
    }
}

/// A [`Plugin`] which applies [`OperationIdLayer`] to every operation.
pub struct OperationIdPlugin(OperationNameFn<fn(OperationId) -> OperationIdLayer>);

impl fmt::Debug for OperationIdPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OperationIdPlugin").field(&"...").finish()
    }
}

impl<P, Op, S, L> Plugin<P, Op, S, L> for OperationIdPlugin
where
    Op: OperationShape,
{
    type Service = S;
    type Layer = Stack<L, OperationIdLayer>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        <OperationNameFn<fn(OperationId) -> OperationIdLayer> as Plugin<P, Op, S, L>>::map(&self.0, input)
    }
}

/// An extension trait on [`PluginPipeline`] allowing the application of [`OperationIdPlugin`].
///
/// See [`module`](crate::extension) documentation for more info.
pub trait OperationIdExt<P> {
    /// Apply the [`OperationIdPlugin`], which inserts the [`OperationId`] into every [`http::Response`].
    fn insert_operation_extension(self) -> PluginPipeline<PluginStack<OperationIdPlugin, P>>;
}

impl<P> OperationIdExt<P> for PluginPipeline<P> {
    fn insert_operation_extension(self) -> PluginPipeline<PluginStack<OperationIdPlugin, P>> {
        let plugin = OperationIdPlugin(plugin_from_operation_name_fn(|shape_id| {
            OperationIdLayer(shape_id.clone())
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
        let ext = OperationId::new(value).unwrap();

        assert_eq!(ext.absolute(), value);
        assert_eq!(ext.namespace(), "com.amazonaws.ebs");
        assert_eq!(ext.name(), "CompleteSnapshot");
    }

    #[test]
    fn ext_reject() {
        let value = "CompleteSnapshot";
        assert_eq!(
            OperationId::new(value).unwrap_err(),
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

        // Check for `OperationId`.
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        let expected = OperationId::new(DummyOp::NAME).unwrap();
        let actual = response.extensions().get::<OperationId>().unwrap();
        assert_eq!(*actual, expected);
    }
}
