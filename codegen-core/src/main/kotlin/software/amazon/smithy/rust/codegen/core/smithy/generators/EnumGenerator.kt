/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.shouldRedact
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/** EnumGenerator customization sections */
sealed class EnumSection(name: String) : Section(name) {
    abstract val shape: Shape

    /** Hook to add additional attributes to an enum member */
    data class AdditionalMemberAttributes(override val shape: Shape, val definition: EnumDefinition) :
        EnumSection("AdditionalMemberAttributes")

    /** Hook to add additional trait implementations */
    data class AdditionalTraitImpls(override val shape: Shape) : EnumSection("AdditionalTraitImpls")

    /** Hook to add additional enum members */
    data class AdditionalEnumMembers(override val shape: Shape) :
        EnumSection("AdditionalEnumMembers")
}

/** Customizations for EnumGenerator */
abstract class EnumCustomization : NamedCustomization<EnumSection>()

data class EnumGeneratorContext(
    val enumName: String,
    val enumMeta: RustMetadata,
    val enumTrait: EnumTrait,
    val sortedMembers: List<EnumMemberModel>,
)

/**
 * Type of enum to generate
 *
 * In codegen-core, there are only `Infallible` enums. Server adds additional enum types, which
 * is why this class is abstract rather than sealed.
 */
abstract class EnumType {
    /** Returns a writable that implements `From<&str>` and/or `TryFrom<&str>` for the enum */
    abstract fun implFromForStr(context: EnumGeneratorContext): Writable

    /** Returns a writable that implements `FromStr` for the enum */
    abstract fun implFromStr(context: EnumGeneratorContext): Writable

    /** Returns a writable that implements `From<&str>` and/or `TryFrom<&str>` for the unnamed enum */
    abstract fun implFromForStrForUnnamedEnum(context: EnumGeneratorContext): Writable

    /** Returns a writable that implements `FromStr` for the unnamed enum */
    abstract fun implFromStrForUnnamedEnum(context: EnumGeneratorContext): Writable

    /** Optionally adds additional documentation to the `enum` docs */
    open fun additionalDocs(context: EnumGeneratorContext): Writable = writable {}

    /** Optionally adds additional enum members */
    open fun additionalEnumMembers(context: EnumGeneratorContext): Writable = writable {}

    /** Optionally adds match arms to the `as_str` match implementation for named enums */
    open fun additionalAsStrMatchArms(context: EnumGeneratorContext): Writable = writable {}

    /** Optionally add more attributes to the enum */
    open fun additionalEnumAttributes(context: EnumGeneratorContext): List<Attribute> = emptyList()

    /** Optionally add more impls to the enum */
    open fun additionalEnumImpls(context: EnumGeneratorContext): Writable = writable {}
}

/** Model that wraps [EnumDefinition] to calculate and cache values required to generate the Rust enum source. */
class EnumMemberModel(
    private val parentShape: Shape,
    private val definition: EnumDefinition,
    private val symbolProvider: RustSymbolProvider,
    private val customizations: List<EnumCustomization>,
) {
    companion object {
        /**
         * Return the name of a given `enum` variant. Note that this refers to `enum` in the Smithy context
         * where enum is a trait that can be applied to [StringShape] and not in the Rust context of an algebraic data type.
         *
         * Ordinarily, the symbol provider would determine this name, but the enum trait doesn't allow for this.
         *
         * TODO(https://github.com/smithy-lang/smithy-rs/issues/1700): Remove this function when refactoring to EnumShape.
         */
        @Deprecated("This function will go away when we handle EnumShape instead of EnumTrait")
        fun toEnumVariantName(
            symbolProvider: RustSymbolProvider,
            parentShape: Shape,
            definition: EnumDefinition,
        ): MaybeRenamed? {
            val name = definition.name.orNull()?.toPascalCase() ?: return null
            // Create a fake member shape for symbol look up until we refactor to use EnumShape
            val fakeMemberShape =
                MemberShape.builder().id(parentShape.id.withMember(name)).target("smithy.api#String").build()
            val symbol = symbolProvider.toSymbol(fakeMemberShape)
            return MaybeRenamed(symbol.name, symbol.renamedFrom())
        }
    }
    // Because enum variants always start with an upper case letter, they will never
    // conflict with reserved words (which are always lower case), therefore, we never need
    // to fall back to raw identifiers

    val value: String get() = definition.value

    fun name(): MaybeRenamed? = toEnumVariantName(symbolProvider, parentShape, definition)

    private fun renderDocumentation(writer: RustWriter) {
        val name =
            checkNotNull(name()) { "cannot generate docs for unnamed enum variants" }
        writer.docWithNote(
            definition.documentation.orNull(),
            name.renamedFrom?.let { renamedFrom ->
                "`::$renamedFrom` has been renamed to `::${name.name}`."
            },
        )
    }

    private fun renderDeprecated(writer: RustWriter) {
        if (definition.isDeprecated) {
            Attribute.Deprecated.render(writer)
        }
    }

    fun derivedName() = checkNotNull(toEnumVariantName(symbolProvider, parentShape, definition)).name

    fun render(writer: RustWriter) {
        renderDocumentation(writer)
        renderDeprecated(writer)
        writer.writeCustomizations(
            customizations,
            EnumSection.AdditionalMemberAttributes(parentShape, definition),
        )
        writer.write("${derivedName()},")
    }
}

private fun RustWriter.docWithNote(
    doc: String?,
    note: String?,
) {
    if (doc.isNullOrBlank() && note.isNullOrBlank()) {
        // If the model doesn't have any documentation for the shape, then suppress the missing docs lint
        // since the lack of documentation is a modeling issue rather than a codegen issue.
        rust("##[allow(missing_docs)] // documentation missing in model")
    } else {
        doc?.also { docs(escape(it)) }
        note?.also {
            // Add a blank line between the docs and the note to visually differentiate
            write("///")
            docs("_Note: ${it}_")
        }
    }
}

open class EnumGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: StringShape,
    private val enumType: EnumType,
    private val customizations: List<EnumCustomization>,
) {
    companion object {
        /** Name of the function on the enum impl to get a vec of value names */
        const val VALUES = "values"
    }

    private val enumTrait: EnumTrait = shape.expectTrait()
    private val symbol: Symbol = symbolProvider.toSymbol(shape)
    private val context =
        EnumGeneratorContext(
            enumName = symbol.name,
            enumMeta = symbol.expectRustMetadata(),
            enumTrait = enumTrait,
            sortedMembers =
                enumTrait.values.sortedBy { it.value }
                    .map { EnumMemberModel(shape, it, symbolProvider, customizations) },
        )

    fun render(writer: RustWriter) {
        enumType.additionalEnumAttributes(context).forEach { attribute ->
            attribute.render(writer)
        }
        if (enumTrait.hasNames()) {
            writer.renderNamedEnum()
        } else {
            writer.renderUnnamedEnum()
        }
        enumType.additionalEnumImpls(context)(writer)
        writer.writeCustomizations(customizations, EnumSection.AdditionalTraitImpls(shape))

        if (shape.shouldRedact(model)) {
            writer.renderDebugImplForSensitiveEnum()
        }
    }

    private fun RustWriter.renderNamedEnum() {
        // pub enum Blah { V1, V2, .. }
        renderEnum()
        insertTrailingNewline()
        // impl From<str> for Blah { ... }
        enumType.implFromForStr(context)(this)
        // impl FromStr for Blah { ... }
        enumType.implFromStr(context)(this)
        insertTrailingNewline()
        // impl Blah { pub fn as_str(&self) -> &str
        implBlock(
            asStrImpl =
                writable {
                    rustBlock("match self") {
                        context.sortedMembers.forEach { member ->
                            rawRust(
                                """${context.enumName}::${member.derivedName()} => ${member.value.dq()},
                                """,
                            )
                        }
                        enumType.additionalAsStrMatchArms(context)(this)
                    }
                },
        )
        rustTemplate(
            """
            impl #{AsRef}<str> for ${context.enumName} {
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }
            """,
            *preludeScope,
        )
    }

    private fun RustWriter.renderUnnamedEnum() {
        documentShape(shape, model)
        deprecatedShape(shape)
        context.enumMeta.render(this)
        rust("struct ${context.enumName}(String);")
        implBlock(
            asStrImpl =
                writable {
                    rust("&self.0")
                },
        )
        rustTemplate(
            """
            impl #{AsRef}<str> for ${context.enumName} {
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }
            """,
            *preludeScope,
        )
        // impl From<str> for Blah { ... }
        enumType.implFromForStrForUnnamedEnum(context)(this)
        // impl FromStr for Blah { ... }
        enumType.implFromStrForUnnamedEnum(context)(this)
    }

    private fun RustWriter.renderEnum() {
        enumType.additionalDocs(context)(this)

        val renamedWarning =
            context.sortedMembers.mapNotNull { it.name() }.filter { it.renamedFrom != null }.joinToString("\n") {
                val previousName = it.renamedFrom!!
                "`${context.enumName}::$previousName` has been renamed to `::${it.name}`."
            }
        docWithNote(
            shape.getTrait<DocumentationTrait>()?.value,
            renamedWarning.ifBlank { null },
        )
        deprecatedShape(shape)

        context.enumMeta.render(this)
        rustBlock("enum ${context.enumName}") {
            context.sortedMembers.forEach { member -> member.render(this) }
            writeCustomizations(
                customizations,
                EnumSection.AdditionalEnumMembers(shape),
            )
            enumType.additionalEnumMembers(context)(this)
        }
    }

    private fun RustWriter.implBlock(asStrImpl: Writable) {
        rustTemplate(
            """
            impl ${context.enumName} {
                /// Returns the `&str` value of the enum member.
                pub fn as_str(&self) -> &str {
                    #{asStrImpl:W}
                }
                /// Returns all the `&str` representations of the enum members.
                pub const fn $VALUES() -> &'static [&'static str] {
                    &[#{Values:W}]
                }
            }
            """,
            "asStrImpl" to asStrImpl,
            "Values" to
                writable {
                    rawRust(context.sortedMembers.joinToString(", ") { it.value.dq() })
                },
        )
    }

    /**
     * Manually implement the `Debug` trait for the enum if marked as sensitive.
     *
     * It prints the redacted text regardless of the variant it is asked to print.
     */
    private fun RustWriter.renderDebugImplForSensitiveEnum() {
        rustTemplate(
            """
            impl #{Debug} for ${context.enumName} {
                fn fmt(&self, f: &mut #{StdFmt}::Formatter<'_>) -> #{StdFmt}::Result {
                    ::std::write!(f, $REDACTION)
                }
            }
            """,
            "Debug" to RuntimeType.Debug,
            "StdFmt" to RuntimeType.stdFmt,
        )
    }
}
