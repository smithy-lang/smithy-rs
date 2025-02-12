/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.std
import software.amazon.smithy.rust.codegen.core.smithy.makeMaybeConstrained
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.TraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator

class ConstrainedPythonBlobGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    val writer: RustWriter,
    val shape: BlobShape,
    private val validationExceptionConversionGenerator: ValidationExceptionConversionGenerator,
) {
    val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    val constraintViolation = constraintViolationSymbolProvider.toSymbol(shape)
    private val blobConstraintsInfo: List<BlobLength> =
        listOf(LengthTrait::class.java)
            .mapNotNull { shape.getTrait(it).orNull() }
            .map { BlobLength(it) }
    private val constraintsInfo: List<TraitInfo> = blobConstraintsInfo.map { it.toTraitInfo() }

    fun render() {
        val symbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val blobType = PythonServerRuntimeType.blob(codegenContext.runtimeConfig).toSymbol().rustType()
        renderFrom(symbol, blobType)
        renderTryFrom(symbol, blobType)
    }

    fun renderFrom(
        symbol: Symbol,
        blobType: RustType,
    ) {
        val name = symbol.name
        val inner = blobType.render()
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
            """,
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
            "From" to RuntimeType.From,
        )
    }

    fun renderTryFrom(
        symbol: Symbol,
        blobType: RustType,
    ) {
        val name = symbol.name
        val inner = blobType.render()
        writer.rustTemplate(
            """
            impl #{TryFrom}<$inner> for $name {
                type Error = #{ConstraintViolation};

                fn try_from(value: $inner) -> #{Result}<Self, Self::Error> {
                    value.try_into()
                }
            }
            """,
            "TryFrom" to RuntimeType.TryFrom,
            "ConstraintViolation" to constraintViolation,
            "TryFromChecks" to constraintsInfo.map { it.tryFromCheck }.join("\n"),
            "Result" to std.resolve("result::Result"),
        )
    }
}
