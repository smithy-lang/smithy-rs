/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.Errors
import software.amazon.smithy.rust.codegen.core.smithy.Inputs
import software.amazon.smithy.rust.codegen.core.smithy.Outputs
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

class ServerOperationShapeGenerator(
    private val operations: List<OperationShape>,
    private val codegenContext: CodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val firstOperation = codegenContext.symbolProvider.toSymbol(operations[0])
    private val firstOperationName = firstOperation.name.toPascalCase()
    private val crateName = codegenContext.settings.moduleName.toSnakeCase()

    /**
     * Returns the function signature for an operation handler implementation. Used in the documentation.
     */
    private fun OperationShape.docSignature(): Writable {
        val inputSymbol = symbolProvider.toSymbol(inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(outputShape(model))
        val errorSymbol = errorSymbol(model, symbolProvider, CodegenTarget.SERVER)

        val outputT = if (errors.isEmpty()) {
            outputSymbol.name
        } else {
            "Result<${outputSymbol.name}, ${errorSymbol.name}>"
        }

        return writable {
            if (!errors.isEmpty()) {
                rust("//! ## use $crateName::${Errors.namespace}::${errorSymbol.name};")
            }
            rust(
                """
                //! ## use $crateName::${Inputs.namespace}::${inputSymbol.name};
                //! ## use $crateName::${Outputs.namespace}::${outputSymbol.name};
                //! async fn handler(input: ${inputSymbol.name}) -> $outputT {
                //!     todo!()
                //! }
                """.trimIndent(),
            )
        }
    }

    fun render(writer: RustWriter) {
        if (operations.isEmpty()) {
            return
        }

        writer.rustTemplate(
            """
            //! A collection of zero-sized types (ZSTs) representing each operation defined in the service closure.
            //!
            //! ## Constructing an [`Operation`](#{SmithyHttpServer}::operation::OperationShapeExt)
            //!
            //! To apply middleware to specific operations the [`Operation`](#{SmithyHttpServer}::operation::Operation)
            //! API must be used.
            //!
            //! Using the [`OperationShapeExt`](#{SmithyHttpServer}::operation::OperationShapeExt) trait
            //! implemented on each ZST we can construct an [`Operation`](#{SmithyHttpServer}::operation::Operation)
            //! with appropriate constraints given by Smithy.
            //!
            //! #### Example
            //!
            //! ```no_run
            //! use $crateName::operation_shape::$firstOperationName;
            //! use #{SmithyHttpServer}::operation::OperationShapeExt;
            #{Handler:W}
            //!
            //! let operation = $firstOperationName::from_handler(handler)
            //!     .layer(todo!("Provide a layer implementation"));
            //! ```
            //!
            //! ## Use as Marker Structs
            //!
            //! The [plugin system](#{SmithyHttpServer}::plugin) also makes use of these ZSTs to parameterize
            //! [`Plugin`](#{SmithyHttpServer}::plugin::Plugin) implementations. The traits, such as
            //! [`OperationShape`](#{SmithyHttpServer}::operation::OperationShape) can be used to provide
            //! operation specific information to the [`Layer`](#{Tower}::Layer) being applied.
            """.trimIndent(),
            "SmithyHttpServer" to
                ServerCargoDependency.SmithyHttpServer(codegenContext.runtimeConfig).toType(),
            "Tower" to ServerCargoDependency.Tower.toType(),
            "Handler" to operations[0].docSignature(),
        )
        for (operation in operations) {
            ServerOperationGenerator(codegenContext, operation).render(writer)
        }
    }
}
