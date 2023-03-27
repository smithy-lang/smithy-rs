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
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.doubleQuote
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.shouldRedact

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

        /** Name of the opaque struct that is inner data for the generated [UnknownVariant]. */
        const val UnknownVariantValue = "UnknownVariantValue"

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

        if (shape.shouldRedact(model)) {
            renderDebugImplForSensitiveEnum()
        }
    }

    private fun renderUnnamedEnum() {
        writer.documentShape(shape, model)
        writer.deprecatedShape(shape)
        RenderSerdeAttribute.writeAttributes(writer)
        SensitiveWarning.addDoc(writer, shape)
        meta.render(writer)
        writer.write("struct $enumName(String);")
        writer.rustBlock("impl $enumName") {
            docs("Returns the `&str` value of the enum member.")
            rustBlock("pub fn as_str(&self) -> &str") {
                rust("&self.0")
            }

            docs("Returns all the `&str` representations of the enum members.")
            rustBlock("pub const fn $Values() -> &'static [&'static str]") {
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
        target.ifClient {
            writer.renderForwardCompatibilityNote(enumName, sortedMembers, UnknownVariant, UnknownVariantValue)
        }

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
        RenderSerdeAttribute.writeAttributes(writer)
        SensitiveWarning.addDoc(writer, shape)
        meta.render(writer)
        writer.rustBlock("enum $enumName") {
            sortedMembers.forEach { member -> member.render(writer) }
            target.ifClient {
                docs("`$UnknownVariant` contains new variants that have been added since this code was generated.")
                rust("$UnknownVariant(#T)", unknownVariantValue())
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

                    target.ifClient {
                        rust("$enumName::$UnknownVariant(value) => value.as_str()")
                    }
                }
            }

            rust("/// Returns all the `&str` values of the enum members.")
            rustBlock("pub const fn $Values() -> &'static [&'static str]") {
                withBlock("&[", "]") {
                    val memberList = sortedMembers.joinToString(", ") { it.value.doubleQuote() }
                    write(memberList)
                }
            }
        }
    }

    private fun unknownVariantValue(): RuntimeType {
        return RuntimeType.forInlineFun(UnknownVariantValue, RustModule.Types) {
            docs(
                """
                Opaque struct used as inner data for the `Unknown` variant defined in enums in
                the crate

                While this is not intended to be used directly, it is marked as `pub` because it is
                part of the enums that are public interface.
                """.trimIndent(),
            )
            // adding serde features here adds attribute to the end of the file for some reason
            meta.render(this)
            rust("struct $UnknownVariantValue(pub(crate) String);")
            rustBlock("impl $UnknownVariantValue") {
                // The generated as_str is not pub as we need to prevent users from calling it on this opaque struct.
                rustBlock("pub(crate) fn as_str(&self) -> &str") {
                    rust("&self.0")
                }
            }
        }
    }

    /**
     * Manually implement the `Debug` trait for the enum if marked as sensitive.
     *
     * It prints the redacted text regardless of the variant it is asked to print.
     */
    private fun renderDebugImplForSensitiveEnum() {
        writer.rustTemplate(
            """
            impl #{Debug} for $enumName {
                fn fmt(&self, f: &mut #{StdFmt}::Formatter<'_>) -> #{StdFmt}::Result {
                    write!(f, $REDACTION)
                }
            }
            """,
            "Debug" to RuntimeType.Debug,
            "StdFmt" to RuntimeType.stdFmt,
        )
    }

    protected open fun renderFromForStr() {
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.From) {
            rustBlock("fn from(s: &str) -> Self") {
                rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        rust("""${member.value.dq()} => $enumName::${member.derivedName()},""")
                    }
                    rust("other => $enumName::$UnknownVariant(#T(other.to_owned()))", unknownVariantValue())
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

/**
 * Generate the rustdoc describing how to write a match expression against a generated enum in a
 * forward-compatible way.
 */
private fun RustWriter.renderForwardCompatibilityNote(
    enumName: String, sortedMembers: List<EnumMemberModel>,
    unknownVariant: String, unknownVariantValue: String,
) {
    docs(
        """
        When writing a match expression against `$enumName`, it is important to ensure
        your code is forward-compatible. That is, if a match arm handles a case for a
        feature that is supported by the service but has not been represented as an enum
        variant in a current version of SDK, your code should continue to work when you
        upgrade SDK to a future version in which the enum does include a variant for that
        feature.
        """.trimIndent(),
    )
    docs("")
    docs("Here is an example of how you can make a match expression forward-compatible:")
    docs("")
    docs("```text")
    rust("/// ## let ${enumName.lowercase()} = unimplemented!();")
    rust("/// match ${enumName.lowercase()} {")
    sortedMembers.mapNotNull { it.name() }.forEach { member ->
        rust("///     $enumName::${member.name} => { /* ... */ },")
    }
    rust("""///     other @ _ if other.as_str() == "NewFeature" => { /* handles a case for `NewFeature` */ },""")
    rust("///     _ => { /* ... */ },")
    rust("/// }")
    docs("```")
    docs(
        """
        The above code demonstrates that when `${enumName.lowercase()}` represents
        `NewFeature`, the execution path will lead to the second last match arm,
        even though the enum does not contain a variant `$enumName::NewFeature`
        in the current version of SDK. The reason is that the variable `other`,
        created by the `@` operator, is bound to
        `$enumName::$unknownVariant($unknownVariantValue("NewFeature".to_owned()))`
        and calling `as_str` on it yields `"NewFeature"`.
        This match expression is forward-compatible when executed with a newer
        version of SDK where the variant `$enumName::NewFeature` is defined.
        Specifically, when `${enumName.lowercase()}` represents `NewFeature`,
        the execution path will hit the second last match arm as before by virtue of
        calling `as_str` on `$enumName::NewFeature` also yielding `"NewFeature"`.
        """.trimIndent(),
    )
    docs("")
    docs(
        """
        Explicitly matching on the `$unknownVariant` variant should
        be avoided for two reasons:
        - The inner data `$unknownVariantValue` is opaque, and no further information can be extracted.
        - It might inadvertently shadow other intended match arms.
        """.trimIndent(),
    )
}
