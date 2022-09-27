/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.asType
import software.amazon.smithy.rust.codegen.client.rustlang.documentShape
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

class ServerOperationGenerator(
    coreCodegenContext: CoreCodegenContext,
    private val operation: OperationShape,
) {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        )
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val model = coreCodegenContext.model

    private val operationName = symbolProvider.toSymbol(operation).name.toPascalCase()
    private val operationId = operation.id

    /** Returns `std::convert::Infallible` if the model provides no errors. */
    private fun operationError(): Writable = writable {
        if (operation.errors.isEmpty()) {
            rust("std::convert::Infallible")
        } else {
            rust("crate::error::${operationName}Error")
        }
    }

    fun render(writer: RustWriter) {
        writer.documentShape(operation, model)

        val generator = ServerHttpSensitivityGenerator(model, operation, runtimeConfig)
        val requestFmt = generator.requestFmt()
        val responseFmt = generator.responseFmt()

        writer.rustTemplate(
            """
            pub struct $operationName;

            impl #{SmithyHttpServer}::operation::OperationShape for $operationName {
                const NAME: &'static str = "${operationId.toString().replace("#", "##")}";

                type Input = crate::input::${operationName}Input;
                type Output = crate::output::${operationName}Output;
                type Error = #{Error:W};
            }

            impl #{SmithyHttpServer}::logging::sensitivity::Sensitivity for $operationName {
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
            "Error" to operationError(),
            "RequestValue" to requestFmt.value,
            "RequestType" to requestFmt.type,
            "ResponseValue" to responseFmt.value,
            "ResponseType" to responseFmt.type,
            *codegenScope,
        )
        // Adds newline to end of render
        writer.rust("")
    }
}
