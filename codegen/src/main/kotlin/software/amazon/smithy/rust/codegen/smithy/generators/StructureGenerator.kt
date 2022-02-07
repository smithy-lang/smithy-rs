/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asDeref
import software.amazon.smithy.rust.codegen.rustlang.asRef
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.isCopy
import software.amazon.smithy.rust.codegen.rustlang.isDeref
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.error.ErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.renamedFrom
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
    private val errorTrait = shape.getTrait<ErrorTrait>()
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val accessorMembers: List<MemberShape> = when (errorTrait) {
        null -> members
        // Let the ErrorGenerator render the error message accessor if this is an error struct
        else -> members.filter { "message" != symbolProvider.toMemberName(it) }
    }
    private val name = symbolProvider.toSymbol(shape).name

    fun render() {
        renderStructure()
        errorTrait?.also { errorTrait ->
            ErrorGenerator(symbolProvider, writer, shape, errorTrait).render()
        }
    }

    fun renderServer() {
        renderStructure()
        errorTrait?.also { errorTrait ->
            ErrorGenerator(symbolProvider, writer, shape, errorTrait).renderServer()
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
                    val fieldValue = redactIfNecessary(
                        member, model, "self.$memberName"
                    )
                    rust(
                        "formatter.field(${memberName.dq()}, &$fieldValue);",
                    )
                }
                rust("formatter.finish()")
            }
        }
    }

    private fun renderStructureImpl() {
        if (accessorMembers.isEmpty()) {
            return
        }
        writer.rustBlock("impl $name") {
            // Render field accessor methods
            forEachMember(accessorMembers) { member, memberName, memberSymbol ->
                renderMemberDoc(member, memberSymbol)
                val memberType = memberSymbol.rustType()
                val returnType = when {
                    memberType.isCopy() -> memberType
                    memberType is RustType.Option && memberType.member.isDeref() -> memberType.asDeref()
                    memberType.isDeref() -> memberType.asDeref().asRef()
                    else -> memberType.asRef()
                }
                rustBlock("pub fn $memberName(&self) -> ${returnType.render()}") {
                    when {
                        memberType.isCopy() -> rust("self.$memberName")
                        memberType is RustType.Option && memberType.member.isDeref() -> rust("self.$memberName.as_deref()")
                        memberType is RustType.Option -> rust("self.$memberName.as_ref()")
                        memberType.isDeref() -> rust("use std::ops::Deref; self.$memberName.deref()")
                        else -> rust("&self.$memberName")
                    }
                }
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
            forEachMember(members) { member, memberName, memberSymbol ->
                renderMemberDoc(member, memberSymbol)
                memberSymbol.expectRustMetadata().render(this)
                write("$memberName: #T,", symbolProvider.toSymbol(member))
            }
        }

        renderStructureImpl()
        renderDebugImpl()
    }

    private fun RustWriter.forEachMember(
        toIterate: List<MemberShape>,
        block: RustWriter.(MemberShape, String, Symbol) -> Unit
    ) {
        toIterate.forEach { member ->
            val memberName = symbolProvider.toMemberName(member)
            val memberSymbol = symbolProvider.toSymbol(member)
            block(member, memberName, memberSymbol)
        }
    }

    private fun RustWriter.renderMemberDoc(member: MemberShape, memberSymbol: Symbol) {
        documentShape(
            member,
            model,
            note = memberSymbol.renamedFrom()
                ?.let { oldName -> "This member has been renamed from `$oldName`." }
        )
    }
}
