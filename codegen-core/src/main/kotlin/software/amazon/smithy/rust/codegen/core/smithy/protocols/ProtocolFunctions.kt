/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Similar to [Writable], but provides the function name that is being generated as an argument.
 */
typealias ProtocolFnWritable = RustWriter.(String) -> Unit

/**
 * Utilities for generating protocol serialization/deserialization functions, and common support functions.
 *
 * This class should be used for generating all inline functions from a protocol code generator, as it is
 * responsible for correctly organizing and code splitting those functions into smaller modules.
 */
class ProtocolFunctions(
    private val codegenContext: CodegenContext,
) {
    companion object {
        val serDeModule = RustModule.pubCrate("protocol_serde")

        fun crossOperationFn(
            fnName: String,
            block: ProtocolFnWritable,
        ): RuntimeType =
            RuntimeType.forInlineFun(fnName, serDeModule) {
                block(fnName)
            }
    }

    private enum class FnType {
        Serialize,
        Deserialize,
    }

    /**
     * Generate a serialize function for a protocol, and return a runtime type that references it
     *
     * Example:
     * ```
     * // Generate the function
     * val serializeFn = ProtocolFunctions(codegenContext).serializeFn(myStruct) { fnName ->
     *     rust("fn $fnName(...) { ... }")
     * }
     * // Call the generated function
     * rust("#T(...)", serializeFn)
     * ```
     */
    fun serializeFn(
        shape: Shape,
        fnNameSuffix: String? = null,
        block: ProtocolFnWritable,
    ): RuntimeType = serDeFn(FnType.Serialize, shape, serDeModule, block, fnNameSuffix)

    /**
     * Generate a deserialize function for a protocol, and return a runtime type that references it
     *
     * Example:
     * ```
     * // Generate the function
     * val deserializeFn = ProtocolFunctions(codegenContext).deserializeFn(myStruct) { fnName ->
     *     rust("fn $fnName(...) { ... }")
     * }
     * // Call the generated function
     * rust("#T(...)", deserializeFn)
     * ```
     */
    fun deserializeFn(
        shape: Shape,
        fnNameSuffix: String? = null,
        block: ProtocolFnWritable,
    ): RuntimeType = serDeFn(FnType.Deserialize, shape, serDeModule, block, fnNameSuffix)

    private fun serDeFn(
        fnType: FnType,
        shape: Shape,
        parentModule: RustModule.LeafModule,
        block: ProtocolFnWritable,
        fnNameSuffix: String? = null,
    ): RuntimeType {
        val moduleName = codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape)
        val fnBaseName = codegenContext.symbolProvider.shapeFunctionName(codegenContext.serviceShape, shape)
        val suffix = fnNameSuffix?.let { "_$it" } ?: ""
        val fnName =
            RustReservedWords.escapeIfNeeded(
                when (fnType) {
                    FnType.Deserialize -> "de_$fnBaseName$suffix"
                    FnType.Serialize -> "ser_$fnBaseName$suffix"
                },
            )
        return serDeFn(moduleName, fnName, parentModule, block)
    }

    private fun serDeFn(
        moduleName: String,
        fnName: String,
        parentModule: RustModule.LeafModule,
        block: ProtocolFnWritable,
    ): RuntimeType {
        val additionalAttributes =
            when {
                // Some SDK models have maps with names prefixed with `__mapOf__`, which become `__map_of__`,
                // and the Rust compiler warning doesn't like multiple adjacent underscores.
                moduleName.contains("__") || fnName.contains("__") -> listOf(Attribute.AllowNonSnakeCase)
                else -> emptyList()
            }
        return RuntimeType.forInlineFun(
            fnName,
            RustModule.pubCrate(moduleName, parent = parentModule, additionalAttributes = additionalAttributes),
        ) {
            block(fnName)
        }
    }
}

/** Creates a module name for a ser/de function. */
fun RustSymbolProvider.shapeModuleName(
    serviceShape: ServiceShape?,
    shape: Shape,
): String =
    RustReservedWords.escapeIfNeeded(
        "shape_" +
            when (shape) {
                is MemberShape -> model.expectShape(shape.container)
                else -> shape
            }.contextName(serviceShape).toSnakeCase(),
        EscapeFor.ModuleName,
    )

/** Creates a unique name for a ser/de function. */
fun RustSymbolProvider.shapeFunctionName(
    serviceShape: ServiceShape?,
    shape: Shape,
): String {
    val extras =
        "".letIf(shape.hasTrait<SyntheticOutputTrait>()) {
            it + "_output"
        }.letIf(shape.hasTrait<SyntheticInputTrait>()) { it + "_input" }
    val containerName =
        when (shape) {
            is MemberShape -> model.expectShape(shape.container).contextName(serviceShape).toSnakeCase()
            else -> shape.contextName(serviceShape).toSnakeCase()
        } + extras
    return when (shape) {
        is MemberShape -> shape.memberName.toSnakeCase()
        is DocumentShape -> "document"
        else -> containerName
    }
}

fun RustSymbolProvider.nestedAccessorName(
    serviceShape: ServiceShape?,
    prefix: String,
    root: Shape,
    path: List<MemberShape>,
): String {
    val base = shapeFunctionName(serviceShape, root)
    val rest = path.joinToString("_") { toMemberName(it) }
    return "${prefix}lens_${base}_$rest"
}
