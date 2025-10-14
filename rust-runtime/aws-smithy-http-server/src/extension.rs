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

use std::hash::Hash;
use std::{fmt, fmt::Debug, future::Future, ops::Deref, pin::Pin, task::Context, task::Poll};

use futures_util::ready;
use futures_util::TryFuture;
use thiserror::Error;
use tower::Service;

use http;

use crate::operation::OperationShape;
use crate::plugin::{HttpMarker, HttpPlugins, Plugin, PluginStack};
use crate::shape_id::ShapeId;

pub use crate::request::extension::{Extension, MissingExtension};

/// Extension type used to store information about Smithy operations in HTTP responses.
/// This extension type is inserted, via the [`OperationExtensionPlugin`], whenever it has been correctly determined
/// that the request should be routed to a particular operation. The operation handler might not even get invoked
/// because the request fails to deserialize into the modeled operation input.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationExtension(pub ShapeId);

/// An error occurred when parsing an absolute operation shape ID.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    #[error("# was not found - missing namespace")]
    MissingNamespace,
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

/// A [`Plugin`] which applies [`OperationExtensionService`] to every operation.
pub struct OperationExtensionPlugin;

impl fmt::Debug for OperationExtensionPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OperationExtensionPlugin").field(&"...").finish()
    }
}

impl<Ser, Op, T> Plugin<Ser, Op, T> for OperationExtensionPlugin
where
    Op: OperationShape,
{
    type Output = OperationExtensionService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        OperationExtensionService {
            inner,
            operation_extension: OperationExtension(Op::ID),
        }
    }
}

impl HttpMarker for OperationExtensionPlugin {}

/// An extension trait on [`HttpPlugins`] allowing the application of [`OperationExtensionPlugin`].
///
/// See [`module`](crate::extension) documentation for more info.
pub trait OperationExtensionExt<CurrentPlugin> {
    /// Apply the [`OperationExtensionPlugin`], which inserts the [`OperationExtension`] into every [`http::Response`].
    fn insert_operation_extension(self) -> HttpPlugins<PluginStack<OperationExtensionPlugin, CurrentPlugin>>;
}

impl<CurrentPlugin> OperationExtensionExt<CurrentPlugin> for HttpPlugins<CurrentPlugin> {
    fn insert_operation_extension(self) -> HttpPlugins<PluginStack<OperationExtensionPlugin, CurrentPlugin>> {
        self.push(OperationExtensionPlugin)
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
    use tower::{service_fn, Layer, ServiceExt};

    use crate::{plugin::PluginLayer, protocol::rest_json_1::RestJson1};

    use super::*;

    #[test]
    fn ext_accept() {
        let value = "com.amazonaws.ebs#CompleteSnapshot";
        let ext = ShapeId::new(
            "com.amazonaws.ebs#CompleteSnapshot",
            "com.amazonaws.ebs",
            "CompleteSnapshot",
        );

        assert_eq!(ext.absolute(), value);
        assert_eq!(ext.namespace(), "com.amazonaws.ebs");
        assert_eq!(ext.name(), "CompleteSnapshot");
    }

    #[tokio::test]
    async fn plugin() {
        struct DummyOp;

        impl OperationShape for DummyOp {
            const ID: ShapeId = ShapeId::new(
                "com.amazonaws.ebs#CompleteSnapshot",
                "com.amazonaws.ebs",
                "CompleteSnapshot",
            );

            type Input = ();
            type Output = ();
            type Error = ();
        }

        // Apply `Plugin`.
        let plugins = HttpPlugins::new().insert_operation_extension();

        // Apply `Plugin`s `Layer`.
        let layer = PluginLayer::new::<RestJson1, DummyOp>(plugins);
        let svc = service_fn(|_: http::Request<()>| async { Ok::<_, ()>(http::Response::new(())) });
        let svc = layer.layer(svc);

        // Check for `OperationExtension`.
        let response = svc.oneshot(http::Request::new(())).await.unwrap();
        let expected = DummyOp::ID;
        let actual = response.extensions().get::<OperationExtension>().unwrap();
        assert_eq!(actual.0, expected);
    }
}
