/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.utils.CaseUtils

// TODO(maybe): extract struct generation from Smithy shapes to support generating body objects
// TODO: generate builders; 1d
class StructureGenerator(
    val model: Model,
    private val symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape
) {
    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }
    fun render() {
        renderStructure()
        val errorTrait = shape.getTrait(ErrorTrait::class.java)
        errorTrait.map {
            val errorGenerator = ErrorGenerator(model, symbolProvider, writer, shape, it)
            errorGenerator.render()
        }
    }

    private fun renderStructure() {
        val symbol = symbolProvider.toSymbol(shape)
        // TODO(maybe): Pull derive info from the symbol so that the symbol provider can alter things as necessary; 4h
        writer.write("#[non_exhaustive]")
        writer.write("#[derive(Debug, PartialEq, Clone)]")
        val blockWriter = writer.openBlock("pub struct ${symbol.name} {")
        sortedMembers.forEach { member ->
            val memberName = symbolProvider.toMemberName(member)
            blockWriter.write("pub $memberName: \$T,", symbolProvider.toSymbol(member)) }
        blockWriter.closeBlock("}")
    }
}

// String extensions
fun String.toSnakeCase(): String {
    return CaseUtils.toSnakeCase(this)
}

fun String.toPascalCase(): String {
    return CaseUtils.toSnakeCase(this).let { CaseUtils.toPascalCase(it) }
}
