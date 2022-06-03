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
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class ServerEnumGenerator(
    model: Model,
    symbolProvider: RustSymbolProvider,
    constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    private val writer: RustWriter,
    shape: StringShape,
    enumTrait: EnumTrait,
) : EnumGenerator(model, symbolProvider, writer, shape, enumTrait) {
    override var target: CodegenTarget = CodegenTarget.SERVER
    private val unknownVariantSymbol = constraintViolationSymbolProvider.toSymbol(shape)

    override fun renderFromForStr() {
        writer.rust(
            """
            pub mod ${enumName.toSnakeCase()} {
                ##[derive(Debug, PartialEq)]
                pub struct ConstraintViolation(pub(crate) String);
            }
            """
        )
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.TryFrom) {
            rust("type Error = #T;", unknownVariantSymbol)
            rustBlock("fn try_from(s: &str) -> Result<Self, <Self as #T<&str>>::Error>", RuntimeType.TryFrom) {
                rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        rust("${member.value.dq()} => Ok($enumName::${member.derivedName()}),")
                    }
                    rust("_ => Err(#T(s.to_owned()))", unknownVariantSymbol)
                }
            }
        }
        writer.rustTemplate(
            """
            impl #{TryFrom}<String> for $enumName {
                type Error = #{UnknownVariantSymbol};
                fn try_from(s: String) -> std::result::Result<Self, <Self as #{TryFrom}<String>>::Error> {
                    s.as_str().try_into()
                }
            }
            """,
            "TryFrom" to RuntimeType.TryFrom,
            "UnknownVariantSymbol" to unknownVariantSymbol
        )
    }

    override fun renderFromStr() {
        writer.rustTemplate(
            """
            impl std::str::FromStr for $enumName {
                type Err = #{UnknownVariantSymbol};

                fn from_str(s: &str) -> std::result::Result<Self, <Self as std::str::FromStr>::Err> {
                    Self::try_from(s)
                }
            }
            """,
            "UnknownVariantSymbol" to unknownVariantSymbol
        )
    }
}
