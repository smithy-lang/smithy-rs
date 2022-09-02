/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ResourceShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class ServerServiceGeneratorV2(
    coreCodegenContext: CoreCodegenContext,
    private val service: ServiceShape,
    private val protocol: ServerProtocol,
) {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "Http" to CargoDependency.Http.asType(),
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
            "Tower" to CargoDependency.Tower.asType(),
        )
    private val model = coreCodegenContext.model
    private val symbolProvider = coreCodegenContext.symbolProvider

    private val serviceName = service.id.name
    private val builderName = "${serviceName}Builder"

    private val resourceOperationShapes = service
        .resources
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? ResourceShape }
        .flatMap { it.allOperations }
        .mapNotNull { model.getShape(it).orNull() }
        .mapNotNull { it as? OperationShape }
    private val operationShapes = service.operations.mapNotNull { model.getShape(it).orNull() }.mapNotNull { it as? OperationShape }
    private val allOperationShapes = resourceOperationShapes + operationShapes

    private fun builderGenerics(): Sequence<String> = sequence {
        for (index in 1..allOperationShapes.size) {
            yield("Op$index")
        }
    }

    private fun builderFieldNames(): Sequence<String> = sequence {
        for (operation in allOperationShapes) {
            val field = RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operation).name.toSnakeCase())
            yield(field)
        }
    }

    private fun operationStructNames(): Sequence<String> = sequence {
        for (operation in allOperationShapes) {
            yield(symbolProvider.toSymbol(operation).name.toPascalCase())
        }
    }

    private fun builderFields(): Writable = writable {
        val zipped = builderFieldNames().zip(builderGenerics())
        for ((name, type) in zipped) {
            rust("$name: $type,")
        }
    }

    private fun builderDef(): Writable = writable {
        rustTemplate(
            """
            /// The service builder for [`$serviceName`].
            ///
            /// Constructed via [`$serviceName::builder`].
            pub struct $builderName<${builderGenerics().joinToString(",")}, Modifier = #{SmithyHttpServer}::build_modifier::Identity> {
                #{Fields:W}
                modifier: Modifier
            }
            """,
            "Fields" to builderFields(),
            *codegenScope,
        )
    }

    private fun builderSetters(): Writable = writable {
        for ((index, pair) in builderFieldNames().zip(operationStructNames()).withIndex()) {
            val (fieldName, structName) = pair
            val replacedGenerics = builderGenerics().withIndex().map { (innerIndex, item) ->
                if (innerIndex == index) {
                    "NewOp"
                } else {
                    item
                }
            }

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
                /// Sets the [`$structName`](crate::operations::$structName) operation.
                ///
                /// This should be an [`Operation`](#{SmithyHttpServer}::operation::Operation) created from
                /// [`$structName`](crate::operations::$structName) using either
                /// [`OperationShape::from_handler`](#{SmithyHttpServer}::operation::OperationShapeExt::from_handler) or
                /// [`OperationShape::from_service`](#{SmithyHttpServer}::operation::OperationShapeExt::from_service).
                pub fn $fieldName<NewOp>(self, value: NewOp) -> $builderName<${replacedGenerics.joinToString(",")}> {
                    $builderName {
                        #{SwitchedFields:W}
                        modifier: self.modifier
                    }
                }
                """,
                "SwitchedFields" to switchedFields,
                *codegenScope,
            )

            // Adds newline to between setters
            rust("")
        }
    }

    private fun extensionTypes(): Sequence<String> = sequence {
        for (index in 1..allOperationShapes.size) {
            yield("Exts$index")
        }
    }

    private fun buildConstraints(): Writable = writable {
        for (tuple in allOperationShapes.asSequence().zip(builderGenerics()).zip(extensionTypes())) {
            val (first, exts) = tuple
            val (operation, type) = first
            // TODO(Relax): The `Error = Infallible` is an excess requirement to stay at parity with existing builder.
            rustTemplate(
                """
                $type: #{SmithyHttpServer}::operation::Upgradable<
                    #{Marker},
                    crate::operations::${symbolProvider.toSymbol(operation).name.toPascalCase()},
                    $exts,
                    B,
                    Modifier
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

    private fun builderImpl(): Writable = writable {
        val generics = builderGenerics().joinToString(",")
        val router = protocol
            .routerConstruction(
                service,
                builderFieldNames()
                    .map {
                        writable { rustTemplate("self.$it.upgrade(&self.modifier)") }
                    }
                    .asIterable(),
                model,
            )
        rustTemplate(
            """
            impl<$generics> $builderName<$generics> {
                #{Setters:W}
            }

            impl<$generics, Modifier> $builderName<$generics, Modifier> {
                pub fn build<B, ${extensionTypes().joinToString(",")}>(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<B>>
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
            "Setters" to builderSetters(),
            "BuildConstraints" to buildConstraints(),
            "Router" to router,
            *codegenScope,
        )
    }

    private fun structDef(): Writable = writable {
        val documentation = service.getTrait<DocumentationTrait>()?.value
        if (documentation != null) {
            docs(documentation.replace("#", "##"))
        }

        rustTemplate(
            """
            pub struct $serviceName<S> {
                router: #{SmithyHttpServer}::routing::routers::RoutingService<#{Router}<S>, #{Protocol}>,
            }
            """,
            "Router" to protocol.routerType(),
            "Protocol" to protocol.markerStruct(),
            *codegenScope,
        )
    }

    private fun notSetGenerics(): Writable = writable {
        for (index in 1..allOperationShapes.size) {
            rustTemplate("#{SmithyHttpServer}::operation::OperationNotSet,", *codegenScope)
        }
    }

    private fun notSetFields(): Writable = writable {
        for (fieldName in builderFieldNames()) {
            rustTemplate(
                "$fieldName: #{SmithyHttpServer}::operation::OperationNotSet,",
                *codegenScope,
            )
        }
    }

    private fun structImpl(): Writable = writable {
        rustTemplate(
            """
            impl $serviceName<()> {
                /// Constructs a builder for [`$serviceName`].
                pub fn builder() -> $builderName<#{NotSetGenerics:W}> {
                    $builderName {
                        #{NotSetFields:W}
                        modifier: #{SmithyHttpServer}::build_modifier::Identity
                    }
                }
            }
            """,
            "NotSetGenerics" to notSetGenerics(),
            "NotSetFields" to notSetFields(),
            *codegenScope,
        )
    }

    private fun structServiceImpl(): Writable = writable {
        rustTemplate(
            """
            impl<B, S> #{Tower}::Service<#{Http}::Request<B>> for $serviceName<S>
            where
                S: #{Tower}::Service<http::Request<B>, Response = http::Response<#{SmithyHttpServer}::body::BoxBody>> + Clone,
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
            *codegenScope,
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            #{Builder:W}

            #{BuilderImpl:W}

            #{Struct:W}

            #{StructImpl:W}

            #{StructServiceImpl:W}
            """,
            "Builder" to builderDef(),
            "BuilderImpl" to builderImpl(),
            "Struct" to structDef(),
            "StructImpl" to structImpl(),
            "StructServiceImpl" to structServiceImpl(),
        )
    }
}
