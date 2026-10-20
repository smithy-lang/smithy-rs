/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

class ServerOperationGenerator(
    private val operation: OperationShape,
    private val codegenContext: CodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to
                ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
        )
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model

    private val operationNames = ServerOperationNames(symbolProvider.toSymbol(operation))
    private val operationName = operationNames.markerType
    private val operationId = operation.id
    private val schemaSerde = (codegenContext as? ServerCodegenContext)?.usesSchemaHttpSerde == true

    /** Returns `std::convert::Infallible` if the model provides no errors. */
    private fun operationError(): Writable =
        writable {
            if (operation.errors.isEmpty()) {
                rust("std::convert::Infallible")
            } else {
                // Name comes from [ServerOperationErrorGenerator].
                rust("crate::error::${symbolProvider.toSymbol(operation).name}Error")
            }
        }

    fun render(writer: RustWriter) {
        writer.documentShape(operation, model)

        val generator = ServerHttpSensitivityGenerator(model, operation, runtimeConfig)
        val requestFmt = generator.requestFmt()
        val responseFmt = generator.responseFmt()

        val operationIdAbsolute = operationId.toString().replace("#", "##")
        // OperationShape is shared with legacy plugins; schema IDs live in SchemaOperationShape::SCHEMA.
        val shapeIdType = "#{SmithyHttpServer}::shape_id::ShapeId"
        val id =
            if (schemaSerde) {
                writable {
                    rustTemplate(
                        """
                        #{SmithyHttpServer}::shape_id::ShapeId::from_schema(
                            <Self as #{SmithyHttpServer}::operation::SchemaOperationShape>::SCHEMA.shape_id()
                        )
                        """,
                        *codegenScope,
                    )
                }
            } else {
                writable {
                    rustTemplate(
                        "#{SmithyHttpServer}::shape_id::ShapeId::new(${operationIdAbsolute.dq()}, ${operationId.namespace.dq()}, ${operationId.name.dq()})",
                        *codegenScope,
                    )
                }
            }
        writer.rustTemplate(
            """
            pub struct $operationName;

            impl #{SmithyHttpServer}::operation::OperationShape for $operationName {
                const ID: $shapeIdType = #{OperationId};

                type Input = #{Input:W};
                type Output = crate::output::${operationName}Output;
                type Error = #{Error:W};
            }

            impl #{SmithyHttpServer}::instrumentation::sensitivity::Sensitivity for $operationName {
                type RequestFmt = #{RequestType:W};
                type ResponseFmt = #{ResponseType:W};

                fn request_fmt() -> Self::RequestFmt {
                    #{RequestValue:W}
                }

                fn response_fmt() -> Self::ResponseFmt {
                    #{ResponseValue:W}
                }
            }
            """,
            "Input" to
                writable {
                    rust("crate::input::${operationName}Input")
                },
            "Error" to operationError(),
            "OperationId" to id,
            "RequestValue" to requestFmt.value,
            "RequestType" to requestFmt.type,
            "ResponseValue" to responseFmt.value,
            "ResponseType" to responseFmt.type,
            *codegenScope,
        )
        if (schemaSerde) {
            writer.rustTemplate(
                """
                impl #{SmithyHttpServer}::operation::SchemaOperationShape for $operationName {
                    const SCHEMA: &'static #{SmithyHttpServer}::schema::OperationSchema<'static> = &crate::schema::operations::${operationNames.schemaConst};
                }
                """,
                *codegenScope,
            )
        }
        // Adds newline to end of render
        writer.rust("")
    }
}
