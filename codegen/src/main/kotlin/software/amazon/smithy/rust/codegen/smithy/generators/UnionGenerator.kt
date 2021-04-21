/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.util.toPascalCase
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class UnionGenerator(
    val model: Model,
    private val symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    private val shape: UnionShape
) {

    fun render() {
        renderUnion()
    }

    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }
    private fun renderUnion() {
        val symbol = symbolProvider.toSymbol(shape)
        val containerMeta = symbol.expectRustMetadata()
        containerMeta.render(writer)
        writer.rustBlock("enum ${symbol.name}") {
            sortedMembers.forEach { member ->
                val memberSymbol = symbolProvider.toSymbol(member)
                documentShape(member, model)
                memberSymbol.expectRustMetadata().renderAttributes(this)
                write("${member.memberName.toPascalCase()}(#T),", symbolProvider.toSymbol(member))
            }
        }
        writer.rustBlock("impl ${symbol.name}") {
            sortedMembers.forEach { member ->
                val memberSymbol = symbolProvider.toSymbol(member)
                val funcNamePart = member.memberName.toSnakeCase().removeSuffix("_value")
                val variantName = member.memberName.toPascalCase()

                writer.rustBlock("pub fn as_$funcNamePart(&self) -> Option<&#T>", memberSymbol) {
                    write("if let ${symbol.name}::$variantName(val) = &self { Some(&val) } else { None }")
                }
                writer.rustBlock("pub fn is_$funcNamePart(&self) -> bool") {
                    write("self.as_$funcNamePart().is_some()")
                }
            }
        }
    }
}
