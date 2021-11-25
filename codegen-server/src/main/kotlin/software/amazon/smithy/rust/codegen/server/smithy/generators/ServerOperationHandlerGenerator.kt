/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * ServerOperationHandlerGenerator
 */
class ServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operations: List<OperationShape>,
) {
    private val service = codegenContext.serviceShape
    private val symbolProvider = codegenContext.symbolProvider
    private val operationNames = operations.map { symbolProvider.toSymbol(it).name.toSnakeCase() }
    private val codegenScope = arrayOf(
        "PinProject" to ServerCargoDependency.PinProject.asType(),
        "Tower" to ServerCargoDependency.Tower.asType(),
        "FuturesUtil" to ServerCargoDependency.FuturesUtil.asType(),
    )

    fun render(writer: RustWriter) {
        renderStaticRust(writer)
    }

    fun renderStaticRust(writer: RustWriter) {
        writer.rustTemplate(
            """
            use std::future::Future;

            use aws_smithy_http_server::body::{box_body, BoxBody};
            use aws_smithy_http_server::{opaque_future, Extension};
            use aws_smithy_http_server::routing::request_spec::{
                PathAndQuerySpec, PathSegment, PathSpec, QuerySegment, QuerySpec, UriSpec, RequestSpec
            };
            use axum::extract::FromRequest;
            use axum::extract::RequestParts;
            use axum::response::IntoResponse;
            use #{FuturesUtil}::{
                future::{BoxFuture, Map},
                FutureExt,
            };
            use http::{Request, Response};
            use #{PinProject}::Pin;
            use std::{
                marker::PhantomData,
                convert::Infallible,
                task::{Context, Poll},
            };
            use #{Tower}::Service;
            /// Struct that holds a handler, that is, a function provided by the user that implements the
            /// Smithy operation.
            pub struct OperationHandler<H, B, R, I> {
                handler: H,
                ##[allow(clippy::type_complexity)]
                _marker: PhantomData<fn() -> (B, R, I)>,
            }
            impl<H, B, R, I> Clone for OperationHandler<H, B, R, I>
            where
                H: Clone,
            {
                fn clone(&self) -> Self {
                    Self {
                        handler: self.handler.clone(),
                        _marker: PhantomData,
                    }
                }
            }
            /// Construct an [`OperationHandler`] out of a function implementing the operation.
            pub fn operation<H, B, R, I>(handler: H) -> OperationHandler<H, B, R, I> {
                OperationHandler {
                    handler,
                    _marker: PhantomData,
                }
            }
            impl<H, B, R, I> Service<Request<B>> for OperationHandler<H, B, R, I>
            where
                H: Handler<B, R, I>,
                B: Send + 'static,
            {
                type Response = Response<BoxBody>;
                type Error = Infallible;
                type Future = OperationHandlerFuture;

                ##[inline]
                fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                    Poll::Ready(Ok(()))
                }

                fn call(&mut self, req: Request<B>) -> Self::Future {
                    let future = Handler::call(self.handler.clone(), req).map(Ok::<_, Infallible> as _);
                    OperationHandlerFuture::new(future)
                }
            }
            type WrapResultInResponseFn = fn(Response<BoxBody>) -> Result<Response<BoxBody>, Infallible>;
            opaque_future! {
                /// Response future for [`OperationHandler`].
                pub type OperationHandlerFuture =
                    Map<BoxFuture<'static, Response<BoxBody>>, WrapResultInResponseFn>;
            }
            pub(crate) mod sealed {
                ##![allow(unreachable_pub, missing_docs, missing_debug_implementations)]
                pub trait HiddenTrait {}
                pub struct Hidden;
                impl HiddenTrait for Hidden {}
            }
            ##[axum::async_trait]
            pub trait Handler<B, T, Fut>: Clone + Send + Sized + 'static {
                ##[doc(hidden)]
                type Sealed: sealed::HiddenTrait;

                async fn call(self, req: Request<B>) -> Response<BoxBody>;
            }
            """,
            *codegenScope
        )
    }
}
