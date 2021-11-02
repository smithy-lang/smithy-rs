/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
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
    private val sortedMembers: List<MemberShape> = shape.allMembers.values.sortedBy { symbolProvider.toMemberName(it) }

    fun render() {
        renderUnion()
    }

    private fun renderUnion() {
        writer.documentShape(shape, model)

        val unionSymbol = symbolProvider.toSymbol(shape)
        val containerMeta = unionSymbol.expectRustMetadata()
        containerMeta.render(writer)
        writer.rustBlock("enum ${unionSymbol.name}") {
            sortedMembers.forEach { member ->
                val memberSymbol = symbolProvider.toSymbol(member)
                documentShape(member, model)
                memberSymbol.expectRustMetadata().renderAttributes(this)
                write("${member.memberName.toPascalCase()}(#T),", symbolProvider.toSymbol(member))
            }
        }
        writer.rustBlock("impl ${unionSymbol.name}") {
            sortedMembers.forEach { member ->
                val memberSymbol = symbolProvider.toSymbol(member)
                val variantName = symbolProvider.toMemberName(member).toPascalCase()
                val funcNamePart = variantName.toSnakeCase()

                if (sortedMembers.size == 1) {
                    Attribute.Custom("allow(irrefutable_let_patterns)").render(this)
                }
                rust("/// Tries to convert the enum instance into [`$variantName`](#T::$variantName).", unionSymbol)
                rust("/// Returns `Err(&Self)` if it can't be converted.")
                rustBlock("pub fn as_$funcNamePart(&self) -> std::result::Result<&#T, &Self>", memberSymbol) {
                    rust("if let ${unionSymbol.name}::$variantName(val) = &self { Ok(&val) } else { Err(&self) }")
                }
                rust("/// Returns true if this is a [`$variantName`](#T::$variantName).", unionSymbol)
                rustBlock("pub fn is_$funcNamePart(&self) -> bool") {
                    rust("self.as_$funcNamePart().is_ok()")
                }
            }
        }
    }
}
