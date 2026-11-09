/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asDeref
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.innerReference
import software.amazon.smithy.rust.codegen.core.rustlang.isCopy
import software.amazon.smithy.rust.codegen.core.rustlang.isDeref
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.core.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.shape
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticImplDisplayTrait
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.core.util.shouldRedact

/** StructureGenerator customization sections */
sealed class StructureSection(name: String) : Section(name) {
    abstract val shape: StructureShape

    /** Hook to add additional fields to the structure */
    data class AdditionalFields(override val shape: StructureShape) : StructureSection("AdditionalFields")

    /** Hook to add additional fields to the `Debug` impl */
    data class AdditionalDebugFields(override val shape: StructureShape, val formatterName: String) :
        StructureSection("AdditionalDebugFields")

    /** Hook to add additional trait impls to the structure */
    data class AdditionalTraitImpls(override val shape: StructureShape, val structName: String) :
        StructureSection("AdditionalTraitImpls")
}

/** Customizations for StructureGenerator */
abstract class StructureCustomization : NamedCustomization<StructureSection>()

data class StructSettings(val flattenVecAccessors: Boolean)

open class StructureGenerator(
    val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
    private val customizations: List<StructureCustomization>,
    private val structSettings: StructSettings,
) {
    companion object {
        /** Reserved struct member names */
        val structureMemberNameMap: Map<String, String> =
            mapOf(
                "build" to "build_value",
                "builder" to "builder_value",
                "default" to "default_value",
            )
    }

    private val errorTrait = shape.getTrait<ErrorTrait>()
    protected val members: List<MemberShape> = shape.allMembers.values.toList()
    private val accessorMembers: List<MemberShape> =
        when (errorTrait) {
            null -> members
            // Let the ErrorGenerator render the error message accessor if this is an error struct
            else -> members.filter { "message" != symbolProvider.toMemberName(it) }
        }
    protected val name: String = symbolProvider.toSymbol(shape).name

    fun render() {
        renderStructure()
    }

    /**
     * Render a custom debug implementation
     * When [SensitiveTrait] support is required, render a custom debug implementation to redact sensitive data
     */
    private fun renderDebugImpl() {
        val lifetime = shape.lifetimeDeclaration(symbolProvider)
        writer.rustBlock(
            "impl ${shape.lifetimeDeclaration(symbolProvider)} #T for $name $lifetime",
            RuntimeType.Debug,
        ) {
            writer.rustBlock("fn fmt(&self, f: &mut #1T::Formatter<'_>) -> #1T::Result", RuntimeType.stdFmt) {
                rust("""let mut formatter = f.debug_struct(${name.dq()});""")

                members.forEach { member ->
                    val memberName = symbolProvider.toMemberName(member)
                    // If the struct is marked sensitive all fields get redacted, otherwise each field is determined on its own
                    val fieldValue =
                        if (shape.shouldRedact(model)) {
                            REDACTION
                        } else {
                            member.redactIfNecessary(
                                model,
                                "self.$memberName",
                            )
                        }

                    rust(
                        "formatter.field(${memberName.dq()}, &$fieldValue);",
                    )
                }
                writeCustomizations(customizations, StructureSection.AdditionalDebugFields(shape, "formatter"))
                rust("formatter.finish()")
            }
        }
    }

    private fun renderImplDisplayIfSyntheticImplDisplayTraitApplied() {
        if (shape.getTrait<SyntheticImplDisplayTrait>() == null) {
            return
        }

        val lifetime = shape.lifetimeDeclaration(symbolProvider)
        writer.rustBlock(
            "impl ${shape.lifetimeDeclaration(symbolProvider)} #T for $name $lifetime",
            RuntimeType.Display,
        ) {
            writer.rustBlock("fn fmt(&self, f: &mut #1T::Formatter<'_>) -> #1T::Result", RuntimeType.stdFmt) {
                write("""::std::write!(f, "$name {{")?;""")

                var separator = ""
                for (index in members.indices) {
                    val member = members[index]
                    val memberName = symbolProvider.toMemberName(member)
                    val memberSymbol = symbolProvider.toSymbol(member)

                    val shouldRedact = shape.shouldRedact(model) || member.shouldRedact(model)
                    // If the shape is redacted then each member shape will be redacted.
                    if (shouldRedact) {
                        write("""::std::write!(f, "$separator$memberName={}", $REDACTION)?;""")
                    } else {
                        val variable = ValueExpression.Reference("&self.$memberName")

                        val target = model.expectShape(member.target)
                        when (target) {
                            is DocumentShape,  is BlobShape, is MapShape, is ListShape -> {
                                // Just print the member field name but not the value.
                                if (memberSymbol.isOptional()) {
                                    rustBlockTemplate("if let #{Some}(_) = ${variable.asRef()}", *preludeScope) {
                                        write("""::std::write!(f, "$separator$memberName=Some()")?;""")
                                    }
                                    rustBlock("else") {
                                        write("""::std::write!(f, "$separator$memberName=None")?;""")
                                    }
                                } else {
                                    write("""::std::write!(f, "$separator$memberName=")?;""")
                                }
                            }
                            else -> {
                                if (memberSymbol.isOptional()) {
                                    rustBlockTemplate("if let #{Some}(inner) = ${variable.asRef()}", *preludeScope) {
                                        write("""::std::write!(f, "$separator$memberName=Some({})", inner)?;""")
                                    }
                                    rustBlock("else") {
                                        write("""::std::write!(f, "$separator$memberName=None")?;""")
                                    }
                                } else {
                                    write("""::std::write!(f, "$separator$memberName={}", ${variable.asRef()})?;""")
                                }
                            }
                        }
                    }

                    if (separator.isEmpty()) {
                        separator = ", "
                    }
                }

                write("""::std::write!(f, "}}")""")
            }
        }
    }

    private fun renderStructureImpl() {
        if (accessorMembers.isEmpty()) {
            return
        }
        writer.rustBlock(
            "impl ${shape.lifetimeDeclaration(symbolProvider)} $name ${
                shape.lifetimeDeclaration(
                    symbolProvider,
                )
            }",
        ) {
            // Render field accessor methods
            forEachMember(accessorMembers) { member, memberName, memberSymbol ->
                val memberType = memberSymbol.rustType()
                var unwrapOrDefault = false
                val returnType =
                    when {
                        // Automatically flatten vecs
                        structSettings.flattenVecAccessors && memberType is RustType.Option && memberType.stripOuter<RustType.Option>() is RustType.Vec -> {
                            unwrapOrDefault = true
                            memberType.stripOuter<RustType.Option>().asDeref().asRef()
                        }

                        memberType.isCopy() -> memberType
                        memberType is RustType.Option && memberType.member.isDeref() -> memberType.asDeref()
                        memberType.isDeref() -> memberType.asDeref().asRef()
                        else -> memberType.asRef()
                    }
                writer.renderMemberDoc(member, memberSymbol)
                if (unwrapOrDefault) {
                    // Add a newline
                    writer.docs("")
                    writer.docs("If no value was sent for this field, a default will be set. If you want to determine if no value was sent, use `.$memberName.is_none()`.")
                }
                writer.deprecatedShape(member)
                writer.rustBlock("pub fn $memberName(&self) -> ${returnType.render()}") {
                    when {
                        memberType.isCopy() -> rust("self.$memberName")
                        memberType is RustType.Option && memberType.member.isDeref() -> rust("self.$memberName.as_deref()")
                        memberType is RustType.Option -> rust("self.$memberName.as_ref()")
                        memberType.isDeref() -> rust("use std::ops::Deref; self.$memberName.deref()")
                        else -> rust("&self.$memberName")
                    }
                    if (unwrapOrDefault) {
                        rust(".unwrap_or_default()")
                    }
                }
            }
        }
    }

    open fun renderStructureMember(
        writer: RustWriter,
        member: MemberShape,
        memberName: String,
        memberSymbol: Symbol,
    ) {
        writer.renderMemberDoc(member, memberSymbol)
        writer.deprecatedShape(member)
        memberSymbol.expectRustMetadata().render(writer)
        writer.write("$memberName: #T,", memberSymbol)
    }

    open fun renderStructure() {
        val symbol = symbolProvider.toSymbol(shape)
        val containerMeta = symbol.expectRustMetadata()
        writer.documentShape(shape, model)
        writer.deprecatedShape(shape)
        containerMeta.render(writer)

        writer.rustBlock("struct $name ${shape.lifetimeDeclaration(symbolProvider)}") {
            writer.forEachMember(members) { member, memberName, memberSymbol ->
                renderStructureMember(writer, member, memberName, memberSymbol)
            }
            writeCustomizations(customizations, StructureSection.AdditionalFields(shape))
        }

        renderStructureImpl()
        if (!containerMeta.hasDebugDerive()) {
            renderDebugImpl()
        }
        renderImplDisplayIfSyntheticImplDisplayTraitApplied()

        writer.writeCustomizations(customizations, StructureSection.AdditionalTraitImpls(shape, name))
    }

    protected fun RustWriter.forEachMember(
        toIterate: List<MemberShape>,
        block: RustWriter.(MemberShape, String, Symbol) -> Unit,
    ) {
        toIterate.forEach { member ->
            val memberName = symbolProvider.toMemberName(member)
            val memberSymbol = symbolProvider.toSymbol(member)
            block(member, memberName, memberSymbol)
        }
    }

    private fun RustWriter.renderMemberDoc(
        member: MemberShape,
        memberSymbol: Symbol,
    ) {
        documentShape(
            member,
            model,
            note =
                memberSymbol.renamedFrom()
                    ?.let { oldName -> "This member has been renamed from `$oldName`." },
        )
    }
}

/**
 * Search for lifetimes used by the members of the struct and generate a declaration.
 * e.g. `<'a, 'b>`
 */
fun StructureShape.lifetimeDeclaration(symbolProvider: RustSymbolProvider): String {
    val lifetimes =
        this.members()
            .mapNotNull { symbolProvider.toSymbol(it).rustType().innerReference()?.let { it as RustType.Reference } }
            .mapNotNull { it.lifetime }
            .toSet().sorted()
    return if (lifetimes.isNotEmpty()) {
        "<${lifetimes.joinToString { "'$it" }}>"
    } else {
        ""
    }
}
