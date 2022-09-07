/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ResourceShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class ServerServiceGeneratorV2(
    coreCodegenContext: CoreCodegenContext,
    private val protocol: ServerProtocol,
) {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "Bytes" to CargoDependency.Bytes.asType(),
            "Http" to CargoDependency.Http.asType(),
            "HttpBody" to CargoDependency.HttpBody.asType(),
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "Tower" to CargoDependency.Tower.asType(),
        )
    private val model = coreCodegenContext.model
    private val symbolProvider = coreCodegenContext.symbolProvider

    private val service = coreCodegenContext.serviceShape
    private val serviceName = service.id.name
    private val builderName = "${serviceName}Builder"

    // Calculate all `operationShape`s contained within the `ServiceShape`.
    private val resourceOperationShapes = service
        .resources
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? ResourceShape }
        .flatMap { it.allOperations }
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? OperationShape }
    private val operationShapes = service.operations.mapNotNull { model.getShape(it).orNull() }.mapNotNull { it as? OperationShape }
    private val allOperationShapes = resourceOperationShapes + operationShapes

    /** Returns the sequence of builder generics: `Op1`, ..., `OpN`. */
    private fun builderGenerics(): Sequence<String> = sequence {
        for (index in 1..allOperationShapes.size) {
            yield("Op$index")
        }
    }

    /** Returns the sequence of extension types: `Ext1`, ..., `ExtN`. */
    private fun extensionTypes(): Sequence<String> = sequence {
        for (index in 1..allOperationShapes.size) {
            yield("Exts$index")
        }
    }

    /** Returns the sequence of field names for the builder. */
    private fun builderFieldNames(): Sequence<String> = sequence {
        for (operation in allOperationShapes) {
            val field = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operation).name.toSnakeCase())
            yield(field)
        }
    }

    /** Returns the sequence of operation struct names. */
    private fun operationStructNames(): Sequence<String> = sequence {
        for (operation in allOperationShapes) {
            yield(symbolProvider.toSymbol(operation).name.toPascalCase())
        }
    }

    /** Returns a `Writable` block of "field: Type" for the builder. */
    private fun builderFields(): Writable = writable {
        val zipped = builderFieldNames().zip(builderGenerics())
        for ((name, type) in zipped) {
            rust("$name: $type,")
        }
    }

    /** Returns a `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun builderSetters(): Writable = writable {
        for ((index, pair) in builderFieldNames().zip(operationStructNames()).withIndex()) {
            val (fieldName, structName) = pair

            // The new generics for the operation setter, using `NewOp` where appropriate.
            val replacedOpGenerics = writable {
                for ((innerIndex, item) in builderGenerics().withIndex()) {
                    if (innerIndex == index) {
                        rust("NewOp")
                    } else {
                        rust(item)
                    }
                    rust(", ")
                }
            }

            // The new generics for the operation setter, using `NewOp` where appropriate.
            val replacedExtGenerics = writable {
                for ((innerIndex, item) in extensionTypes().withIndex()) {
                    if (innerIndex == index) {
                        rust("NewExts")
                    } else {
                        rust(item)
                    }
                    rust(", ")
                }
            }

            // The new generics for the handler setter, using `NewOp` where appropriate.
            val replacedOpServiceGenerics = writable {
                for ((innerIndex, item) in builderGenerics().withIndex()) {
                    if (innerIndex == index) {
                        rustTemplate(
                            """
                            #{SmithyHttpServer}::operation::Operation<#{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, H>>
                            """,
                            *codegenScope,
                        )
                    } else {
                        rust(item)
                    }
                    rust(", ")
                }
            }

            // The assignment of fields, using value where appropriate.
            val switchedFields = writable {
                for ((innerIndex, innerFieldName) in builderFieldNames().withIndex()) {
                    if (index == innerIndex) {
                        rust("$innerFieldName: value,")
                    } else {
                        rust("$innerFieldName: self.$innerFieldName,")
                    }
                }
            }

            rustTemplate(
                """
                /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                ///
                /// This should be a closure satisfying the [`Handler`](#{SmithyHttpServer}::operation::Handler) trait.
                /// See the [operation module documentation](#{SmithyHttpServer}::operation) for more information.
                pub fn $fieldName<H, NewExts>(self, value: H) -> $builderName<#{ReplacedOpServiceGenerics:W} #{ReplacedExtGenerics:W}>
                where
                    H: #{SmithyHttpServer}::operation::Handler<crate::operation_shape::$structName, NewExts>
                {
                    use #{SmithyHttpServer}::operation::OperationShapeExt;
                    self.${fieldName}_operation(crate::operation_shape::$structName::from_handler(value))
                }

                /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                ///
                /// This should be an [`Operation`](#{SmithyHttpServer}::operation::Operation) created from
                /// [`$structName`](crate::operation_shape::$structName) using either
                /// [`OperationShape::from_handler`](#{SmithyHttpServer}::operation::OperationShapeExt::from_handler) or
                /// [`OperationShape::from_service`](#{SmithyHttpServer}::operation::OperationShapeExt::from_service).
                pub fn ${fieldName}_operation<NewOp, NewExts>(self, value: NewOp) -> $builderName<#{ReplacedOpGenerics:W} #{ReplacedExtGenerics:W}>
                {
                    $builderName {
                        #{SwitchedFields:W}
                        _exts: std::marker::PhantomData
                    }
                }
                """,
                "Protocol" to protocol.markerStruct(),
                "SwitchedFields" to switchedFields,
                "ReplacedOpGenerics" to replacedOpGenerics,
                "ReplacedOpServiceGenerics" to replacedOpServiceGenerics,
                "ReplacedExtGenerics" to replacedExtGenerics,
                *codegenScope,
            )

            // Adds newline between setters.
            rust("")
        }
    }

    /** Returns the constraints required for the `build` method. */
    private fun buildConstraints(): Writable = writable {
        for (tuple in allOperationShapes.asSequence().zip(builderGenerics()).zip(extensionTypes())) {
            val (first, exts) = tuple
            val (operation, type) = first
            // TODO(https://github.com/awslabs/smithy-rs/issues/1713#issue-1365169734): The `Error = Infallible` is an
            // excess requirement to stay at parity with existing builder.
            rustTemplate(
                """
                $type: #{SmithyHttpServer}::operation::Upgradable<
                    #{Marker},
                    crate::operation_shape::${symbolProvider.toSymbol(operation).name.toPascalCase()},
                    $exts,
                    B,
                >,
                $type::Service: Clone + Send + 'static,
                <$type::Service as #{Tower}::Service<#{Http}::Request<B>>>::Future: Send + 'static,

                $type::Service: #{Tower}::Service<#{Http}::Request<B>, Error = std::convert::Infallible>,
                """,
                "Marker" to protocol.markerStruct(),
                *codegenScope,
            )
        }
    }

    /** Returns a `Writable` containing the builder struct definition and its implementations. */
    private fun builder(): Writable = writable {
        val structGenerics = (builderGenerics() + extensionTypes().map { "$it = ()" }).joinToString(",")
        val generics = (builderGenerics() + extensionTypes()).joinToString(",")

        // Generate router construction block.
        val router = protocol
            .routerConstruction(
                service,
                builderFieldNames()
                    .map {
                        writable { rustTemplate("self.$it.upgrade()") }
                    }
                    .asIterable(),
                model,
            )
        rustTemplate(
            """
            /// The service builder for [`$serviceName`].
            ///
            /// Constructed via [`$serviceName::builder`].
            pub struct $builderName<$structGenerics> {
                #{Fields:W}
                ##[allow(unused_parens)]
                _exts: std::marker::PhantomData<(${extensionTypes().joinToString(",")})>
            }

            impl<$generics> $builderName<$generics> {
                #{Setters:W}
            }

            impl<$generics> $builderName<$generics> {
                /// Constructs a [`$serviceName`] from the arguments provided to the builder.
                pub fn build<B>(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<B>>
                where
                    #{BuildConstraints:W}
                {
                    let router = #{Router:W};
                    $serviceName {
                        router: #{SmithyHttpServer}::routing::routers::RoutingService::new(router),
                    }
                }
            }
            """,
            "Fields" to builderFields(),
            "Setters" to builderSetters(),
            "BuildConstraints" to buildConstraints(),
            "Router" to router,
            *codegenScope,
        )
    }

    /** Returns a `Writable` comma delimited sequence of `MissingOperation`. */
    private fun notSetGenerics(): Writable = writable {
        for (index in 1..allOperationShapes.size) {
            rustTemplate("#{SmithyHttpServer}::operation::MissingOperation,", *codegenScope)
        }
    }

    /** Returns a `Writable` comma delimited sequence of `builder_field: MissingOperation`. */
    private fun notSetFields(): Writable = writable {
        for (fieldName in builderFieldNames()) {
            rustTemplate(
                "$fieldName: #{SmithyHttpServer}::operation::MissingOperation,",
                *codegenScope,
            )
        }
    }

    /** Returns a `Writable` containing the service struct definition and its implementations. */
    private fun struct(): Writable = writable {
        documentShape(service, model)

        rustTemplate(
            """
            ##[derive(Clone)]
            pub struct $serviceName<S> {
                router: #{SmithyHttpServer}::routing::routers::RoutingService<#{Router}<S>, #{Protocol}>,
            }

            impl $serviceName<()> {
                /// Constructs a builder for [`$serviceName`].
                pub fn builder() -> $builderName<#{NotSetGenerics:W}> {
                    $builderName {
                        #{NotSetFields:W}
                        _exts: std::marker::PhantomData
                    }
                }
            }

            impl<S> $serviceName<S> {
                /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService).
                pub fn into_make_service(self) -> #{SmithyHttpServer}::routing::IntoMakeService<Self> {
                    #{SmithyHttpServer}::routing::IntoMakeService::new(self)
                }

                /// Applies a layer uniformly to all routes.
                pub fn layer<L>(self, layer: &L) -> $serviceName<L::Service>
                where
                    L: #{Tower}::Layer<S>
                {
                    $serviceName {
                        router: self.router.map(|s| s.layer(layer))
                    }
                }
            }

            impl<B, RespB, S> #{Tower}::Service<#{Http}::Request<B>> for $serviceName<S>
            where
                S: #{Tower}::Service<#{Http}::Request<B>, Response = #{Http}::Response<RespB>> + Clone,
                RespB: #{HttpBody}::Body<Data = #{Bytes}::Bytes> + Send + 'static,
                RespB::Error: Into<Box<dyn std::error::Error + Send + Sync>>
            {
                type Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>;
                type Error = S::Error;
                type Future = #{SmithyHttpServer}::routing::routers::RoutingFuture<S, B>;

                fn poll_ready(&mut self, cx: &mut std::task::Context) -> std::task::Poll<Result<(), Self::Error>> {
                    self.router.poll_ready(cx)
                }

                fn call(&mut self, request: #{Http}::Request<B>) -> Self::Future {
                    self.router.call(request)
                }
            }
            """,
            "NotSetGenerics" to notSetGenerics(),
            "NotSetFields" to notSetFields(),
            "Router" to protocol.routerType(),
            "Protocol" to protocol.markerStruct(),
            *codegenScope,
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            #{Builder:W}

            #{Struct:W}
            """,
            "Builder" to builder(),
            "Struct" to struct(),
        )
    }
}
