/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

class ServerServiceGeneratorV2(
    codegenContext: CodegenContext,
    private val protocol: ServerProtocol,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "Bytes" to CargoDependency.Bytes.asType(),
            "Http" to CargoDependency.Http.asType(),
            "HttpBody" to CargoDependency.HttpBody.asType(),
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "Tower" to CargoDependency.Tower.asType(),
        )
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider

    private val service = codegenContext.serviceShape
    private val serviceName = service.id.name.toPascalCase()
    private val builderName = "${serviceName}Builder"

    /** Calculate all `operationShape`s contained within the `ServiceShape`. */
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations = index.getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }

    /** The sequence of builder generics: `Op1`, ..., `OpN`. */
    private val builderOps = (1..operations.size).map { "Op$it" }

    /** The sequence of extension types: `Ext1`, ..., `ExtN`. */
    private val extensionTypes = (1..operations.size).map { "Exts$it" }

    /** The sequence of field names for the builder. */
    private val builderFieldNames = operations.map { RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(it).name.toSnakeCase()) }

    /** The sequence of operation struct names. */
    private val operationStructNames = operations.map { symbolProvider.toSymbol(it).name.toPascalCase() }

    /** A `Writable` block of "field: Type" for the builder. */
    private val builderFields = builderFieldNames.zip(builderOps).map { (name, type) -> "$name: $type" }

    /** A `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun builderSetters(): Writable = writable {
        val pluginType = listOf("Pl")
        for ((index, pair) in builderFieldNames.zip(operationStructNames).withIndex()) {
            val (fieldName, structName) = pair

            // The new generics for the operation setter, using `NewOp` where appropriate.
            val replacedOpGenerics = builderOps.withIndex().map { (innerIndex, item) ->
                if (innerIndex == index) {
                    "NewOp"
                } else {
                    item
                }
            }

            // The new generics for the operation setter, using `NewOp` where appropriate.
            val replacedExtGenerics = extensionTypes.withIndex().map { (innerIndex, item) ->
                if (innerIndex == index) {
                    "NewExts"
                } else {
                    item
                }
            }

            // The new generics for the handler setter, using `NewOp` where appropriate.
            val replacedOpServiceGenerics = builderOps.withIndex().map { (innerIndex, item) ->
                if (innerIndex == index) writable {
                    rustTemplate(
                        """
                        #{SmithyHttpServer}::operation::Operation<#{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, H>>
                        """,
                        *codegenScope,
                    )
                } else {
                    writable(item)
                }
            }

            // The assignment of fields, using value where appropriate.
            val switchedFields = builderFieldNames.withIndex().map { (innerIndex, item) ->
                if (index == innerIndex) {
                    "$item: value"
                } else {
                    "$item: self.$item"
                }
            }

            rustTemplate(
                """
                /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                ///
                /// This should be an async function satisfying the [`Handler`](#{SmithyHttpServer}::operation::Handler) trait.
                /// See the [operation module documentation](#{SmithyHttpServer}::operation) for more information.
                pub fn $fieldName<H, NewExts>(self, value: H) -> $builderName<#{HandlerSetterGenerics:W}>
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
                pub fn ${fieldName}_operation<NewOp, NewExts>(self, value: NewOp) -> $builderName<${(replacedOpGenerics + replacedExtGenerics + pluginType).joinToString(", ")}>
                {
                    $builderName {
                        ${switchedFields.joinToString(", ")},
                        _exts: std::marker::PhantomData,
                        plugin: self.plugin,
                    }
                }
                """,
                "Protocol" to protocol.markerStruct(),
                "HandlerSetterGenerics" to (replacedOpServiceGenerics + ((replacedExtGenerics + pluginType).map { writable(it) })).join(", "),
                *codegenScope,
            )

            // Adds newline between setters.
            rust("")
        }
    }

    /** Returns the constraints required for the `build` method. */
    private val buildConstraints = operations.zip(builderOps).zip(extensionTypes).map { (first, exts) ->
        val (operation, type) = first
        // TODO(https://github.com/awslabs/smithy-rs/issues/1713#issue-1365169734): The `Error = Infallible` is an
        // excess requirement to stay at parity with existing builder.
        writable {
            rustTemplate(
                """
                $type: #{SmithyHttpServer}::operation::Upgradable<
                    #{Marker},
                    crate::operation_shape::${symbolProvider.toSymbol(operation).name.toPascalCase()},
                    $exts,
                    B,
                    Pl,
                >,
                $type::Service: Clone + Send + 'static,
                <$type::Service as #{Tower}::Service<#{Http}::Request<B>>>::Future: Send + 'static,

                $type::Service: #{Tower}::Service<#{Http}::Request<B>, Error = std::convert::Infallible>
                """,
                "Marker" to protocol.markerStruct(),
                *codegenScope,
            )
        }
    }

    /** Returns a `Writable` containing the builder struct definition and its implementations. */
    private fun builder(): Writable = writable {
        val extensionTypesDefault = extensionTypes.map { "$it = ()" }
        val pluginName = "Pl"
        val pluginTypeList = listOf(pluginName)
        val newPluginType = "New$pluginName"
        val pluginTypeDefault = listOf("$pluginName = #{SmithyHttpServer}::plugin::IdentityPlugin")
        val structGenerics = (builderOps + extensionTypesDefault + pluginTypeDefault).joinToString(", ")
        val builderGenerics = (builderOps + extensionTypes + pluginTypeList).joinToString(", ")
        val builderGenericsNoPlugin = (builderOps + extensionTypes).joinToString(", ")

        // Generate router construction block.
        val router = protocol
            .routerConstruction(
                builderFieldNames
                    .map {
                        writable { rustTemplate("self.$it.upgrade(&self.plugin)") }
                    }
                    .asIterable(),
            )
        val setterFields = builderFieldNames.map { item ->
            "$item: self.$item"
        }.joinToString(", ")
        rustTemplate(
            """
            /// The service builder for [`$serviceName`].
            ///
            /// Constructed via [`$serviceName::builder`].
            pub struct $builderName<$structGenerics> {
                ${builderFields.joinToString(", ")},
                ##[allow(unused_parens)]
                _exts: std::marker::PhantomData<(${extensionTypes.joinToString(", ")})>,
                plugin: $pluginName,
            }

            impl<$builderGenerics> $builderName<$builderGenerics> {
                #{Setters:W}
            }

            impl<$builderGenerics> $builderName<$builderGenerics> {
                /// Constructs a [`$serviceName`] from the arguments provided to the builder.
                pub fn build<B>(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<B>>
                where
                    #{BuildConstraints:W}
                {
                    let router = #{Router:W};
                    $serviceName {
                        router: #{SmithyHttpServer}::routers::RoutingService::new(router),
                    }
                }
            }

            impl<$builderGenerics, $newPluginType> #{SmithyHttpServer}::plugin::Pluggable<$newPluginType> for $builderName<$builderGenerics> {
                type Output = $builderName<$builderGenericsNoPlugin, #{SmithyHttpServer}::plugin::PluginStack<$pluginName, $newPluginType>>;
                fn apply(self, plugin: $newPluginType) -> Self::Output {
                    $builderName {
                        $setterFields,
                        _exts: self._exts,
                        plugin: #{SmithyHttpServer}::plugin::PluginStack::new(self.plugin, plugin),
                    }
                }
            }
            """,
            "Setters" to builderSetters(),
            "BuildConstraints" to buildConstraints.join(", "),
            "Router" to router,
            *codegenScope,
        )
    }

    /** A `Writable` comma delimited sequence of `MissingOperation`. */
    private val notSetGenerics = (1..operations.size).map {
        writable { rustTemplate("#{SmithyHttpServer}::operation::MissingOperation", *codegenScope) }
    }

    /** Returns a `Writable` comma delimited sequence of `builder_field: MissingOperation`. */
    private val notSetFields = builderFieldNames.map {
        writable {
            rustTemplate(
                "$it: #{SmithyHttpServer}::operation::MissingOperation",
                *codegenScope,
            )
        }
    }

    /** A `Writable` comma delimited sequence of `DummyOperation`. */
    private val internalFailureGenerics = (1..operations.size).map { writable { rustTemplate("#{SmithyHttpServer}::operation::FailOnMissingOperation", *codegenScope) } }

    /** A `Writable` comma delimited sequence of `builder_field: DummyOperation`. */
    private val internalFailureFields = builderFieldNames.map {
        writable {
            rustTemplate(
                "$it: #{SmithyHttpServer}::operation::FailOnMissingOperation",
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
                router: #{SmithyHttpServer}::routers::RoutingService<#{Router}<S>, #{Protocol}>,
            }

            impl $serviceName<()> {
                /// Constructs a builder for [`$serviceName`].
                pub fn builder() -> $builderName<#{NotSetGenerics:W}> {
                    $builderName {
                        #{NotSetFields:W},
                        _exts: std::marker::PhantomData,
                        plugin: #{SmithyHttpServer}::plugin::IdentityPlugin
                    }
                }

                /// Constructs an unchecked builder for [`$serviceName`].
                ///
                /// This will not enforce that all operations are set, however if an unset operation is used at runtime
                /// it will return status code 500 and log an error.
                pub fn unchecked_builder() -> $builderName<#{InternalFailureGenerics:W}> {
                    $builderName {
                        #{InternalFailureFields:W},
                        _exts: std::marker::PhantomData,
                        plugin: #{SmithyHttpServer}::plugin::IdentityPlugin
                    }
                }
            }

            impl<S> $serviceName<S> {
                /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService).
                pub fn into_make_service(self) -> #{SmithyHttpServer}::routing::IntoMakeService<Self> {
                    #{SmithyHttpServer}::routing::IntoMakeService::new(self)
                }

                /// Applies a [`Layer`](#{Tower}::Layer) uniformly to all routes.
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
                type Future = #{SmithyHttpServer}::routers::RoutingFuture<S, B>;

                fn poll_ready(&mut self, cx: &mut std::task::Context) -> std::task::Poll<Result<(), Self::Error>> {
                    self.router.poll_ready(cx)
                }

                fn call(&mut self, request: #{Http}::Request<B>) -> Self::Future {
                    self.router.call(request)
                }
            }
            """,
            "InternalFailureGenerics" to internalFailureGenerics.join(", "),
            "InternalFailureFields" to internalFailureFields.join(", "),
            "NotSetGenerics" to notSetGenerics.join(", "),
            "NotSetFields" to notSetFields.join(", "),
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
