/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.util.dq

open class ServerEnumGenerator(
    model: Model,
    symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    shape: StringShape,
    enumTrait: EnumTrait,
    private val runtimeConfig: RuntimeConfig,
) : EnumGenerator(model, symbolProvider, writer, shape, enumTrait) {
    override var target: CodegenTarget = CodegenTarget.SERVER
    private val errorStruct = "${enumName}UnknownVariantError"

    override fun renderFromForStr() {
        writer.rust(
            """
            ##[derive(Debug, PartialEq, Eq, Hash)]
            pub struct $errorStruct(String);
            """,
        )
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.TryFrom) {
            write("type Error = $errorStruct;")
            writer.rustBlock("fn try_from(s: &str) -> Result<Self, <$enumName as #T<&str>>::Error>", RuntimeType.TryFrom) {
                writer.rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        write("${member.value.dq()} => Ok($enumName::${member.derivedName()}),")
                    }
                    write("_ => Err($errorStruct(s.to_owned()))")
                }
            }
        }
        writer.rustTemplate(
            """
            impl #{From}<$errorStruct> for #{RequestRejection} {
                fn from(e: $errorStruct) -> Self {
                    Self::EnumVariantNotFound(Box::new(e))
                }
            }
            impl #{StdError} for $errorStruct { }
            impl #{Display} for $errorStruct {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    self.0.fmt(f)
                }
            }
            """,
            "Display" to RuntimeType.Display,
            "From" to RuntimeType.From,
            "StdError" to RuntimeType.StdError,
            "RequestRejection" to ServerRuntimeType.RequestRejection(runtimeConfig),
        )
    }

    override fun renderFromStr() {
        writer.rust(
            """
            impl std::str::FromStr for $enumName {
                type Err = $errorStruct;
                fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                    $enumName::try_from(s)
                }
            }
            """,
        )
    }
}
