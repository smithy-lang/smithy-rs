/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

class ServerOperationShapeGenerator(
    private val operations: List<OperationShape>,
    private val codegenContext: CodegenContext,
) {

    fun render(writer: RustWriter) {
        if (operations.isEmpty()) {
            return
        }

        val firstOperation = codegenContext.symbolProvider.toSymbol(operations[0])
        val firstOperationName = firstOperation.name.toPascalCase()
        val crateName = codegenContext.settings.moduleName.toSnakeCase()

        writer.rustTemplate(
            """
            //! A collection of types representing each operation defined in the service closure.
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
            //! The [plugin system](#{SmithyHttpServer}::plugin) also makes use of these
            //! [zero-sized types](https://doc.rust-lang.org/nomicon/exotic-sizes.html##zero-sized-types-zsts) (ZSTs) to
            //! parameterize [`Plugin`](#{SmithyHttpServer}::plugin::Plugin) implementations. The traits, such as
            //! [`OperationShape`](#{SmithyHttpServer}::operation::OperationShape) can be used to provide
            //! operation specific information to the [`Layer`](#{Tower}::Layer) being applied.
            """.trimIndent(),
            "SmithyHttpServer" to
                ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
            "Tower" to ServerCargoDependency.Tower.toType(),
            "Handler" to DocHandlerGenerator(codegenContext, operations[0], "handler", "//!")::render,
        )
        for (operation in operations) {
            ServerOperationGenerator(codegenContext, operation).render(writer)
        }
    }
}
