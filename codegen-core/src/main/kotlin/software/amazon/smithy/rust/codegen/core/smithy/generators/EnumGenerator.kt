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
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.doubleQuote
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull

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
            Attribute.Custom.deprecated().render(writer)
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
    private val writer: RustWriter,
    protected val shape: StringShape,
    protected val enumTrait: EnumTrait,
) {
    protected val symbol: Symbol = symbolProvider.toSymbol(shape)
    protected val enumName: String = symbol.name
    protected val meta = symbol.expectRustMetadata()
    protected val sortedMembers: List<EnumMemberModel> =
        enumTrait.values.sortedBy { it.value }.map { EnumMemberModel(it, symbolProvider) }
    protected open var target: CodegenTarget = CodegenTarget.CLIENT

    companion object {
        /** Name of the generated unknown enum member name for enums with named members. */
        const val UnknownVariant = "Unknown"

        /** Name of the function on the enum impl to get a vec of value names */
        const val Values = "values"
    }

    open fun render() {
        if (enumTrait.hasNames()) {
            // pub enum Blah { V1, V2, .. }
            renderEnum()
            writer.insertTrailingNewline()
            // impl From<str> for Blah { ... }
            renderFromForStr()
            // impl FromStr for Blah { ... }
            renderFromStr()
            writer.insertTrailingNewline()
            // impl Blah { pub fn as_str(&self) -> &str
            implBlock()
            writer.rustBlock("impl AsRef<str> for $enumName") {
                rustBlock("fn as_ref(&self) -> &str") {
                    rust("self.as_str()")
                }
            }
        } else {
            renderUnnamedEnum()
        }
    }

    private fun renderUnnamedEnum() {
        writer.documentShape(shape, model)
        writer.deprecatedShape(shape)
        meta.render(writer)
        writer.write("struct $enumName(String);")
        writer.rustBlock("impl $enumName") {
            docs("Returns the `&str` value of the enum member.")
            rustBlock("pub fn as_str(&self) -> &str") {
                rust("&self.0")
            }

            docs("Returns all the `&str` representations of the enum members.")
            rustBlock("pub fn $Values() -> &'static [&'static str]") {
                withBlock("&[", "]") {
                    val memberList = sortedMembers.joinToString(", ") { it.value.dq() }
                    rust(memberList)
                }
            }
        }

        writer.rustBlock("impl <T> #T<T> for $enumName where T: #T<str>", RuntimeType.From, RuntimeType.AsRef) {
            rustBlock("fn from(s: T) -> Self") {
                rust("$enumName(s.as_ref().to_owned())")
            }
        }
    }

    private fun renderEnum() {
        val renamedWarning =
            sortedMembers.mapNotNull { it.name() }.filter { it.renamedFrom != null }.joinToString("\n") {
                val previousName = it.renamedFrom!!
                "`$enumName::$previousName` has been renamed to `::${it.name}`."
            }
        writer.docWithNote(
            shape.getTrait<DocumentationTrait>()?.value,
            renamedWarning.ifBlank { null },
        )
        writer.deprecatedShape(shape)

        meta.render(writer)
        writer.rustBlock("enum $enumName") {
            sortedMembers.forEach { member -> member.render(writer) }
            if (target == CodegenTarget.CLIENT) {
                docs("$UnknownVariant contains new variants that have been added since this code was generated.")
                rust("$UnknownVariant(String)")
            }
        }
    }

    private fun implBlock() {
        writer.rustBlock("impl $enumName") {
            rust("/// Returns the `&str` value of the enum member.")
            rustBlock("pub fn as_str(&self) -> &str") {
                rustBlock("match self") {
                    sortedMembers.forEach { member ->
                        rust("""$enumName::${member.derivedName()} => ${member.value.dq()},""")
                    }
                    if (target == CodegenTarget.CLIENT) {
                        rust("$enumName::$UnknownVariant(s) => s.as_ref()")
                    }
                }
            }

            rust("/// Returns all the `&str` values of the enum members.")
            rustBlock("pub fn $Values() -> &'static [&'static str]") {
                withBlock("&[", "]") {
                    val memberList = sortedMembers.joinToString(", ") { it.value.doubleQuote() }
                    write(memberList)
                }
            }
        }
    }

    protected open fun renderFromForStr() {
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.From) {
            rustBlock("fn from(s: &str) -> Self") {
                rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        rust("""${member.value.dq()} => $enumName::${member.derivedName()},""")
                    }
                    rust("other => $enumName::$UnknownVariant(other.to_owned())")
                }
            }
        }
    }

    open fun renderFromStr() {
        writer.rust(
            """
            impl std::str::FromStr for $enumName {
                type Err = std::convert::Infallible;

                fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                    Ok($enumName::from(s))
                }
            }
            """,
        )
    }
}
