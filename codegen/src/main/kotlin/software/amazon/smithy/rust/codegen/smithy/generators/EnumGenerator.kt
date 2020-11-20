/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.symbol.expectRustMetadata
import software.amazon.smithy.rust.codegen.util.doubleQuote

class EnumGenerator(
    symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    shape: StringShape,
    private val enumTrait: EnumTrait
) {
    private val sortedMembers: List<EnumDefinition> = enumTrait.values.sortedBy { it.value }
    private val symbol = symbolProvider.toSymbol(shape)
    private val enumName = symbol.name
    private val meta = symbol.expectRustMetadata()
    companion object {
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
            renderAsStr()
        } else {
            renderUnamedEnum()
        }
    }

    private fun renderUnamedEnum() {
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

        writer.rustBlock("impl <T> \$T<T> for $enumName where T: \$T<str>", RuntimeType.From, RuntimeType.AsRef) {
            writer.rustBlock("fn from(s: T) -> Self") {
                write("$enumName(s.as_ref().to_owned())")
            }
        }
    }

    private fun EnumDefinition.derivedName(): String {
        // Because enum variants always start with an upper case letter, they will never
        // conflict with reserved words (which are always lower case), therefore, we never need
        // to fall back to raw identifiers
        return name.orElse(null)?.toPascalCase() ?: throw IllegalStateException("Enum variants must be named to derive a name. This is a bug.")
    }

    private fun renderEnum() {
        meta.render(writer)
        writer.rustBlock("enum $enumName") {
            sortedMembers.forEach { member ->
                member.documentation.map { setNewlinePrefix("/// ").write(it).setNewlinePrefix("") }
                // use the name, or escape the value
                write("${member.derivedName()},")
            }
            write("Unknown(String)")
        }
    }

    private fun renderAsStr() {
        // TODO: should enums also implement AsRef<str>?
        writer.rustBlock("impl $enumName") {
            writer.rustBlock("pub fn as_str(&self) -> &str") {
                writer.rustBlock("match self") {
                    sortedMembers.forEach { member ->
                        write("""$enumName::${member.derivedName()} => "${member.value}",""")
                    }
                    write("$enumName::Unknown(s) => s.as_ref()")
                }
            }
        }
    }

    private fun renderFromStr() {
        writer.rustBlock("impl <T> \$T<T> for $enumName where T: \$T<str>", RuntimeType.From, RuntimeType.AsRef) {
            writer.rustBlock("fn from(s: T) -> Self") {
                writer.rustBlock("match s.as_ref()") {
                    sortedMembers.forEach { member ->
                        write(""""${member.value}" => $enumName::${member.derivedName()},""")
                    }
                    write("other => $enumName::Unknown(other.to_owned())")
                }
            }
        }
    }
}
