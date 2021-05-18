/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.util.doubleQuote
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase

/** Model that wraps [EnumDefinition] to calculate and cache values required to generate the Rust enum source. */
internal class EnumMemberModel(private val definition: EnumDefinition) {
    // Because enum variants always start with an upper case letter, they will never
    // conflict with reserved words (which are always lower case), therefore, we never need
    // to fall back to raw identifiers
    private val unescapedName: String? = definition.name.orNull()?.toPascalCase()

    val collidesWithUnknown: Boolean = unescapedName == EnumGenerator.UnknownVariant

    /** Enum name with correct case format and collision resolution */
    fun derivedName(): String = when (collidesWithUnknown) {
        // If there is a variant named "Unknown", then rename it to "UnknownValue" so that it
        // doesn't conflict with the code generator's "Unknown" variant that exists for backwards compatibility.
        true -> "UnknownValue"
        else -> checkNotNull(unescapedName) { "Enum variants must be named to derive a name. This is a bug." }
    }

    val value: String get() = definition.value

    private fun renderDocumentation(writer: RustWriter) {
        writer.docWithNote(
            definition.documentation.orNull(),
            when (collidesWithUnknown) {
                true ->
                    "`::${EnumGenerator.UnknownVariant}` has been renamed to `::${EnumGenerator.EscapedUnknownVariant}`. " +
                        "`::${EnumGenerator.UnknownVariant}` refers to additional values that may have been added since " +
                        "this enum was generated."
                else -> null
            }
        )
    }

    fun render(writer: RustWriter) {
        renderDocumentation(writer)
        writer.write("${derivedName()},")
    }
}

private fun RustWriter.docWithNote(doc: String?, note: String?) {
    doc?.also { docs(it) }
    note?.also {
        doc?.also { write("///") }
        docs("**NOTE:** $it")
    }
}

class EnumGenerator(
    private val model: Model,
    symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    private val shape: StringShape,
    private val enumTrait: EnumTrait
) {
    private val symbol = symbolProvider.toSymbol(shape)
    private val enumName = symbol.name
    private val meta = symbol.expectRustMetadata()
    private val sortedMembers: List<EnumMemberModel> = enumTrait.values.sortedBy { it.value }.map(::EnumMemberModel)

    companion object {
        /**
         * For enums with named members, variants with names that collide with the generated unknown enum
         * member get renamed to this [EscapedUnknownVariant] value.
         */
        const val EscapedUnknownVariant = "UnknownValue"
        /** Name of the generated unknown enum member name for enums with named members. */
        const val UnknownVariant = "Unknown"
        /** Name of the function on the enum impl to get a vec of value names */
        const val Values = "values"
    }

    fun render() {
        if (enumTrait.hasNames()) {
            // pub enum Blah { V1, V2, .. }
            renderEnum()
            writer.insertTrailingNewline()
            // impl From<str> for Blah { ... }
            renderFromStr()
            writer.insertTrailingNewline()
            // impl Blah { pub fn as_str(&self) -> &str
            implBlock()
            writer.rustBlock("impl AsRef<str> for $enumName") {
                writer.rustBlock("fn as_ref(&self) -> &str") {
                    rust("self.as_str()")
                }
            }
        } else {
            renderUnamedEnum()
        }
        renderSerde()
    }

    private fun renderUnamedEnum() {
        writer.documentShape(shape, model)
        meta.render(writer)
        writer.write("struct $enumName(String);")
        writer.rustBlock("impl $enumName") {
            writer.rustBlock("pub fn as_str(&self) -> &str") {
                write("&self.0")
            }

            writer.rustBlock("pub fn $Values() -> &'static [&'static str]") {
                withBlock("&[", "]") {
                    val memberList = sortedMembers.joinToString(", ") { it.value.doubleQuote() }
                    write(memberList)
                }
            }
        }

        writer.rustBlock("impl <T> #T<T> for $enumName where T: #T<str>", RuntimeType.From, RuntimeType.AsRef) {
            writer.rustBlock("fn from(s: T) -> Self") {
                write("$enumName(s.as_ref().to_owned())")
            }
        }
    }

    private fun renderEnum() {
        writer.docWithNote(
            shape.getTrait<DocumentationTrait>()?.value,
            when (sortedMembers.any { it.collidesWithUnknown }) {
                true ->
                    "`$enumName::$UnknownVariant` has been renamed to `::$EscapedUnknownVariant`. " +
                        "`$enumName::$UnknownVariant` refers to additional values that may have been added since " +
                        "this enum was generated."
                else -> null
            }
        )

        meta.render(writer)
        writer.rustBlock("enum $enumName") {
            sortedMembers.forEach { member -> member.render(writer) }
            write("$UnknownVariant(String)")
        }
    }

    private fun implBlock() {
        writer.rustBlock("impl $enumName") {
            writer.rustBlock("pub fn as_str(&self) -> &str") {
                writer.rustBlock("match self") {
                    sortedMembers.forEach { member ->
                        write("""$enumName::${member.derivedName()} => "${member.value}",""")
                    }
                    write("$enumName::$UnknownVariant(s) => s.as_ref()")
                }
            }
        }
    }

    private fun renderSerde() {
        writer.rustTemplate(
            """
                impl #{serialize} for $enumName {
                    fn serialize<S>(&self, serializer: S) -> Result<<S as #{serializer}>::Ok, <S as #{serializer}>::Error> where S: #{serializer}{
                        serializer.serialize_str(self.as_str())
                    }
                }

                impl<'de> #{deserialize}<'de> for $enumName {
                    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: #{deserializer}<'de> {
                        let data = <&str>::deserialize(deserializer)?;
                        Ok(Self::from(data))
                    }
                }
            """,
            "serializer" to RuntimeType.Serializer,
            "serialize" to RuntimeType.Serialize,
            "deserializer" to RuntimeType.Deserializer,
            "deserialize" to RuntimeType.Deserialize
        )
    }

    private fun renderFromStr() {
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.From) {
            writer.rustBlock("fn from(s: &str) -> Self") {
                writer.rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        write(""""${member.value}" => $enumName::${member.derivedName()},""")
                    }
                    write("other => $enumName::$UnknownVariant(other.to_owned())")
                }
            }
        }

        writer.rust(
            """
            impl std::str::FromStr for $enumName {
                type Err = std::convert::Infallible;

                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    Ok($enumName::from(s))
                }
            }
            """
        )
    }
}
