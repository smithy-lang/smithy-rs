/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
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
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.shouldRedact

data class EnumGeneratorContext(
    val enumName: String,
    val enumMeta: RustMetadata,
    val enumTrait: EnumTrait,
    val sortedMembers: List<EnumMemberModel>,
)

/** Model that wraps [EnumDefinition] to calculate and cache values required to generate the Rust enum source. */
class EnumMemberModel(private val definition: EnumDefinition, private val symbolProvider: RustSymbolProvider) {
    // Because enum variants always start with an upper case letter, they will never
    // conflict with reserved words (which are always lower case), therefore, we never need
    // to fall back to raw identifiers

    val value: String get() = definition.value

    fun name(): MaybeRenamed? = symbolProvider.toEnumVariantName(definition)

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

    fun derivedName() = checkNotNull(symbolProvider.toEnumVariantName(definition)).name

    fun render(writer: RustWriter) {
        renderDocumentation(writer)
        renderDeprecated(writer)
        writer.write("${derivedName()},")
    }
}

private fun RustWriter.docWithNote(doc: String?, note: String?) {
    if (doc.isNullOrBlank() && note.isNullOrBlank()) {
        // If the model doesn't have any documentation for the shape, then suppress the missing docs lint
        // since the lack of documentation is a modeling issue rather than a codegen issue.
        rust("##[allow(missing_docs)] // documentation missing in model")
    } else {
        doc?.also { docs(escape(it)) }
        note?.also {
            // Add a blank line between the docs and the note to visually differentiate
            doc?.also { write("///") }
            docs("_Note: ${it}_")
        }
    }
}

open class EnumGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: StringShape,
    private val enumType: EnumType,
) {
    companion object {
        /** Name of the function on the enum impl to get a vec of value names */
        const val Values = "values"
    }

    private val enumTrait: EnumTrait = shape.expectTrait()
    private val symbol: Symbol = symbolProvider.toSymbol(shape)
    private val context = EnumGeneratorContext(
        enumName = symbol.name,
        enumMeta = symbol.expectRustMetadata(),
        enumTrait = enumTrait,
        sortedMembers = enumTrait.values.sortedBy { it.value }.map { EnumMemberModel(it, symbolProvider) },
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
            asStrImpl = writable {
                rustBlock("match self") {
                    context.sortedMembers.forEach { member ->
                        rust("""${context.enumName}::${member.derivedName()} => ${member.value.dq()},""")
                    }
                    enumType.additionalAsStrMatchArms(context)(this)
                }
            },
        )
        rust(
            """
            impl AsRef<str> for ${context.enumName} {
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }
            """,
        )
    }

    private fun RustWriter.renderUnnamedEnum() {
        documentShape(shape, model)
        deprecatedShape(shape)
        context.enumMeta.render(this)
        rust("struct ${context.enumName}(String);")
        implBlock(
            asStrImpl = writable {
                rust("&self.0")
            },
        )

        rustTemplate(
            """
            impl<T> #{From}<T> for ${context.enumName} where T: #{AsRef}<str> {
                fn from(s: T) -> Self {
                    ${context.enumName}(s.as_ref().to_owned())
                }
            }
            """,
            "From" to RuntimeType.From,
            "AsRef" to RuntimeType.AsRef,
        )
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
                /// Returns all the `&str` values of the enum members.
                pub const fn $Values() -> &'static [&'static str] {
                    &[#{Values:W}]
                }
            }
            """,
            "asStrImpl" to asStrImpl,
            "Values" to writable {
                rust(context.sortedMembers.joinToString(", ") { it.value.dq() })
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
                    write!(f, $REDACTION)
                }
            }
            """,
            "Debug" to RuntimeType.Debug,
            "StdFmt" to RuntimeType.stdFmt,
        )
    }
}
