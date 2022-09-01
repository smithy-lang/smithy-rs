/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

private fun shapeToStructName(shapeId: ShapeId): String {
    return shapeId.name.toPascalCase()
}

private fun shapeToFieldName(shapeId: ShapeId): String {
    return shapeId.name.toSnakeCase()
}

class ServerServiceGeneratorV2(
    runtimeConfig: RuntimeConfig,
    private val service: ServiceShape,
    private val protocol: ServerProtocol,
) {
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        )

    private val serviceId = service.id

    private fun builderName(): String {
        return "${shapeToStructName(serviceId)}Builder"
    }

    private fun builderGenerics(): Sequence<String> {
        return sequence {
            for (index in 1..service.operations.size) {
                yield("Op$index")
            }
        }
    }

    private fun builderFieldNames(): Sequence<String> {
        return sequence {
            for (operation in service.operations) {
                yield(shapeToFieldName(operation))
            }
        }
    }

    private fun operationStructNames(): Sequence<String> {
        return sequence {
            for (operation in service.operations) {
                yield(shapeToStructName(operation))
            }
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
            /// The service builder for [`${shapeToStructName(serviceId)}`].
            ///
            /// Constructed via [`${shapeToStructName(serviceId)}::builder`].
            pub struct ${builderName()}<${builderGenerics().joinToString(",")}> {
                #{Fields:W}
            }
            """,
            "Fields" to builderFields(),
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
                /// Sets the `$structName` operation.
                ///
                /// This should be an [`Operation`](#{SmithyHttpServer}::operation::Operation) created from [`$structName`](crate::operations::$structName)
                /// using either [`OperationShape::from_handler`](#{SmithyHttpServer}::operation::OperationExt::from_handler) or
                /// [`OperationShape::from_service`](#{SmithyHttpServer}::operation::OperationExt::from_handler).
                pub fn $fieldName<NewOp>(self, value: NewOp) -> ${builderName()}<${
                replacedGenerics.joinToString(
                    ",",
                )
                }> {
                    ${builderName()} {
                        #{SwitchedFields:W}
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

    private fun builderImpl(): Writable = writable {
        val generics = builderGenerics().joinToString(",")
        rustTemplate(
            """
            impl<$generics> ${builderName()}<$generics> {
                #{Setters:W}
            }
            """,
            "Setters" to builderSetters(),
        )
    }

    private fun structDef(): Writable = writable {
        val documentationLines = service.getTrait<DocumentationTrait>()?.value?.lines()
        if (documentationLines != null) {
            for (documentation in documentationLines) {
                rust("/// $documentation")
            }
        }
        rustTemplate(
            """
            pub struct ${shapeToStructName(serviceId)}<S> {
                router: #{SmithyHttpServer}::routing::routers::RoutingService<#{Router}<S>, #{Protocol}>,
            }
            """,
            "Router" to protocol.routerType(),
            "Protocol" to protocol.markerStruct(),
            *codegenScope,
        )
    }

    private fun notSetGenerics(): Writable = writable {
        for (index in 1..service.operations.size) {
            rustTemplate("#{SmithyHttpServer}::operation::OperationNotSet,", *codegenScope)
        }
    }

    private fun notSetFields(): Writable = writable {
        for (operation in service.operations) {
            rustTemplate(
                "${shapeToFieldName(operation)}: #{SmithyHttpServer}::operation::OperationNotSet,",
                *codegenScope,
            )
        }
    }

    private fun structImpl(): Writable = writable {
        rustTemplate(
            """
            impl ${shapeToStructName(serviceId)}<()> {
                /// Constructs a builder for [`${shapeToStructName(serviceId)}`].
                pub fn builder() -> ${builderName()}<#{NotSetGenerics:W}> {
                    ${builderName()} {
                        #{NotSetFields:W}
                    }
                }
            }
            """,
            "NotSetGenerics" to notSetGenerics(), "NotSetFields" to notSetFields(),
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            #{Builder:W}

            #{BuilderImpl:W}

            #{Struct:W}

            #{StructImpl:W}
            """,
            "Builder" to builderDef(),
            "BuilderImpl" to builderImpl(),
            "Struct" to structDef(),
            "StructImpl" to structImpl(),
        )
    }
}
