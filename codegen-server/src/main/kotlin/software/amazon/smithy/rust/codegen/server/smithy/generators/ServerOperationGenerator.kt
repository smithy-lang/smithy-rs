/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.util.getTrait

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

    private val operationName = symbolProvider.toSymbol(operation).name
    private val operationId = operation.id

    private fun renderStructDef(): Writable = writable {
        val documentationLines = operation.getTrait<DocumentationTrait>()?.value?.lines()
        if (documentationLines != null) {
            for (documentation in documentationLines) {
                rust("/// $documentation")
            }
        }

        rust("pub struct $operationName;")
    }

    private fun operationError(): Writable = writable {
        if (operation.errors.isEmpty()) {
            rust("std::convert::Infallible")
        } else {
            rust("crate::error::${operationName}Error")
        }
    }

    private fun renderImpl(): Writable = writable {
        rustTemplate(
            """
            impl #{SmithyHttpServer}::operation::OperationShape for $operationName {
                const NAME: &'static str = "${operationId.toString().replace("#", "##")}";

                type Input = crate::input::${operationName}Input;
                type Output = crate::output::${operationName}Output;
                type Error = #{Error:W};
            }
            """,
            "Error" to operationError(),
            *codegenScope,
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            #{Struct:W}

            #{Impl:W}
            """,
            "Struct" to renderStructDef(),
            "Impl" to renderImpl(),
        )
        // Adds newline to end of render
        writer.rust("")
    }
}
