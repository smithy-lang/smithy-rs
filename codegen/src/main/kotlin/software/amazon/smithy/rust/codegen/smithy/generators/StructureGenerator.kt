/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.error.ErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

fun RustWriter.implBlock(structureShape: Shape, symbolProvider: SymbolProvider, block: RustWriter.() -> Unit) {
    rustBlock("impl ${symbolProvider.toSymbol(structureShape).name}") {
        block(this)
    }
}

fun redactIfNecessary(member: MemberShape, model: Model, safeToPrint: String): String {
    return if (member.getMemberTrait(model, SensitiveTrait::class.java).isPresent) {
        "*** Sensitive Data Redacted ***".dq()
    } else {
        safeToPrint
    }
}

class StructureGenerator(
    val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape
) {
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val name = symbolProvider.toSymbol(shape).name

    fun render() {
        renderStructure()
        shape.getTrait<ErrorTrait>()?.also { errorTrait ->
            ErrorGenerator(model, symbolProvider, writer, shape, errorTrait).render()
        }
    }

    companion object {
        /** Returns whether a structure shape requires a fallible builder to be generated. */
        fun fallibleBuilder(structureShape: StructureShape, symbolProvider: SymbolProvider): Boolean =
            // All inputs should have fallible builders in case a new required field is added in the future
            structureShape.hasTrait<SyntheticInputTrait>() ||
                structureShape
                    .allMembers
                    .values.map { symbolProvider.toSymbol(it) }.any {
                        // If any members are not optional && we can't use a default, we need to
                        // generate a fallible builder
                        !it.isOptional() && !it.canUseDefault()
                    }
    }

    /**
     * Search for lifetimes used by the members of the struct and generate a declaration.
     * e.g. `<'a, 'b>`
     */
    private fun lifetimeDeclaration(): String {
        val lifetimes = members
            .map { symbolProvider.toSymbol(it).rustType() }
            .mapNotNull {
                when (it) {
                    is RustType.Reference -> it.lifetime
                    else -> null
                }
            }.toSet().sorted()
        return if (lifetimes.isNotEmpty()) {
            "<${lifetimes.joinToString { "'$it" }}>"
        } else ""
    }

    /** Render a custom debug implementation
     * When [SensitiveTrait] support is required, render a custom debug implementation to redact sensitive data
     */
    private fun renderDebugImpl() {
        writer.rustBlock("impl ${lifetimeDeclaration()} #T for $name ${lifetimeDeclaration()}", RuntimeType.Debug) {
            writer.rustBlock("fn fmt(&self, f: &mut #1T::Formatter<'_>) -> #1T::Result", RuntimeType.stdfmt) {
                rust("""let mut formatter = f.debug_struct(${name.dq()});""")
                members.forEach { member ->
                    val memberName = symbolProvider.toMemberName(member)
                    rust(
                        "formatter.field(${memberName.dq()}, &${redactIfNecessary(member, model, "self.$memberName")});",
                    )
                }
                rust("formatter.finish()")
            }
        }
    }

    private fun renderStructure() {
        val symbol = symbolProvider.toSymbol(shape)
        val containerMeta = symbol.expectRustMetadata()
        writer.documentShape(shape, model)
        val withoutDebug = containerMeta.derives.copy(derives = containerMeta.derives.derives - RuntimeType.Debug)
        containerMeta.copy(derives = withoutDebug).render(writer)

        writer.rustBlock("struct $name ${lifetimeDeclaration()}") {
            members.forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                writer.documentShape(member, model)
                symbolProvider.toSymbol(member).expectRustMetadata().render(this)
                write("$memberName: #T,", symbolProvider.toSymbol(member))
            }
        }

        renderDebugImpl()
    }
}
