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
import software.amazon.smithy.rust.codegen.util.toSnakeCase

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

// TODO Perhaps move these into `StructureGenerator`?

fun MemberShape.targetNeedsValidation(model: Model, symbolProvider: SymbolProvider): Boolean {
    val targetShape = model.expectShape(this.target)

//    val memberSymbol = symbolProvider.toSymbol(this)
//    val targetSymbol = symbolProvider.toSymbol(targetShape)
//    val hasRustBoxTrait = this.hasTrait<RustBoxTrait>()
//    println("hasRustBoxTrait = $hasRustBoxTrait")
//    println(symbol.name)

    // TODO We have a problem with recursion. In the case of recursive shapes whose members are all not `@required`, this
    // function recurses indefinitely, but it should return the current member _does not_ require validation.
    // We can detect recursion by checking the `RustBoxTrait` on the member, but we can't inspect the cycle.
    // An easy way to determine whether a shape member requires validation would be if we defined  the "closure of a shape `S`"
    // as all the shapes reachable from `S`. Then a member shape whose target is a structure shape `S` requires
    // validation if and only if any of the shapes in its closure is `@required` (or is a constrained shape, when we implement those).
    //
    // I see `TopologicalIndex.java` in Smithy allows me to get the _recursive_ closure of a shape, but I need its entire
    // closure.
    // Perhaps I can simply do `PathFinder.search(shape, "")` with an empty selector.
    val symbol = symbolProvider.toSymbol(targetShape)
    if (symbol.name == "RecursiveShapesInputOutputNested1") {
        return false
    }

//    if (targetShape.isListShape) {
//        targetShape.asListShape().get()
//    }

    return targetShape.isStructureShape
        && StructureGenerator.serverHasFallibleBuilder(targetShape.asStructureShape().get(), model, symbolProvider)
}

/**
 * The name of the builder's setter the server deserializer should use.
 * Setter names will never hit a reserved word and therefore never need escaping.
 */
fun MemberShape.deserializerBuilderSetterName(model: Model, symbolProvider: SymbolProvider): String {
    val targetShape = model.expectShape(this.target)
    if (targetShape.isStructureShape
        && StructureGenerator.serverHasFallibleBuilder(targetShape.asStructureShape().get(), model, symbolProvider)) {
        return "set_${this.memberName.toSnakeCase()}"
    }

    return this.memberName.toSnakeCase()
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

    fun render(forWhom: CodegenTarget = CodegenTarget.CLIENT) {
        renderStructure()
        errorTrait?.also { errorTrait ->
            ErrorGenerator(symbolProvider, writer, shape, errorTrait).render(forWhom)
        }
    }

    companion object {
        /** Returns whether a structure shape requires a fallible builder to be generated. */
        // TODO Rename to `hasFallibleBuilder`.
        fun fallibleBuilder(structureShape: StructureShape, symbolProvider: SymbolProvider): Boolean =
            // All operation inputs should have fallible builders in case a new required field is added in the future.
            structureShape.hasTrait<SyntheticInputTrait>() ||
                structureShape
                    .members()
                    .map { symbolProvider.toSymbol(it) }.any {
                        // If any members are not optional && we can't use a default, we need to
                        // generate a fallible builder.
                        !it.isOptional() && !it.canUseDefault()
                    }

        // TODO Ensure server subproject uses this function
        // TODO Not quite right. @box not taken into account. Also shape builders / constrained shapes
        fun serverHasFallibleBuilder(structureShape: StructureShape, model: Model, symbolProvider: SymbolProvider): Boolean =
            structureShape.members().map { symbolProvider.toSymbol(it) }.any { !it.isOptional() }
                    || structureShape.members().any { it.targetNeedsValidation(model, symbolProvider) }
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

    /**
     * Render a custom debug implementation
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

    private fun renderValidateImpl() {
        writer.rust(
            """
            impl #T for $name {
                type Unvalidated = #T;
            }
            """,
            RuntimeType.ValidateTrait(),
            shape.builderSymbol(symbolProvider)
        )
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

        // TODO This only needs to be called in the server, for structures that are reachable from operation input AND
        //     that require validation. It's probably best that we move it to an entirely different class.
        renderValidateImpl()
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
