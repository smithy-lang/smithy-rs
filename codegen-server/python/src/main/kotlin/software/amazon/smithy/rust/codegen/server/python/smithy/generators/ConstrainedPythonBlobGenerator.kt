/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator

class ConstrainedPythonBlobGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    val writer: RustWriter,
    val shape: BlobShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider

    fun render() {
        val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val name = symbol.name
        val inner = PythonServerRuntimeType.blob(codegenContext.runtimeConfig).toSymbol().rustType().render()
        writer.rustTemplate(
            """
            impl #{From}<$inner> for #{MaybeConstrained} {
                fn from(value: $inner) -> Self {
                    Self::Unconstrained(value.into())
                }
            }

            impl #{From}<$name> for $inner {
                fn from(value: $name) -> Self {
                    value.into_inner().into()
                }
            }

            impl #{From}<$inner> for $name {
                fn from(value: $inner) -> Self {
                    value.into()
                }
            }
            """,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "From" to RuntimeType.From,
        )
    }
}
