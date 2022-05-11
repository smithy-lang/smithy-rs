/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.CodegenMode
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.util.dq

class ServerEnumGenerator(
    model: Model,
    symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    shape: StringShape,
    enumTrait: EnumTrait,
    mode: CodegenMode,
    private val runtimeConfig: RuntimeConfig,
) : EnumGenerator(model, symbolProvider, writer, shape, enumTrait, mode,) {
    override fun renderFromForStr() {
        val errorStruct = "${enumName}UnknownVariantError"
        writer.write("##[derive(Debug, PartialEq, Eq, Hash)]")
        writer.write("pub struct $errorStruct(String);")
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.TryFrom) {
            write("type Error = $errorStruct;")
            writer.rustBlock("fn try_from(s: &str) -> Result<Self, <$enumName as #T<&str>>::Error>", RuntimeType.TryFrom) {
                writer.rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        write("""${member.value.dq()} => Ok($enumName::${member.derivedName()}),""")
                    }
                    write("_ => Err($errorStruct(s.to_owned()))")
                }
            }
        }
        writer.rustBlock("impl #T<$errorStruct> for #T", RuntimeType.From, ServerRuntimeType.RequestRejection(runtimeConfig)) {
            writer.rustBlock("fn from(e: $errorStruct) -> Self") {
                write("Self::EnumVariantNotFound(Box::new(e))")
            }
        }
        writer.rustBlock("impl From<$errorStruct> for #T", RuntimeType.jsonDeserialize(runtimeConfig)) {
            writer.rustBlock("fn from(e: $errorStruct) -> Self") {
                write("""Self::custom(format!("unknown variant {}", e))""")
            }
        }
        writer.rustBlock("impl std::error::Error for $errorStruct") {}
        writer.rustBlock("impl std::fmt::Display for $errorStruct") {
            writer.rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                write("self.0.fmt(f)")
            }
        }
    }
}
