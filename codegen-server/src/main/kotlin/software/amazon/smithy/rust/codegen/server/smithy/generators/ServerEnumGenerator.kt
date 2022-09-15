/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.client.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.client.util.dq
import software.amazon.smithy.rust.codegen.client.util.expectTrait

open class ServerEnumGenerator(
    val codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    shape: StringShape,
) : EnumGenerator(codegenContext.model, codegenContext.symbolProvider, writer, shape, shape.expectTrait()) {
    override var target: CodegenTarget = CodegenTarget.SERVER

    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val constraintViolationSymbolProvider =
        with(codegenContext.constraintViolationSymbolProvider) {
            if (publicConstrainedTypes) {
                this
            } else {
                PubCrateConstraintViolationSymbolProvider(this)
            }
        }
    private val constraintViolationSymbol = constraintViolationSymbolProvider.toSymbol(shape)
    private val constraintViolationName = constraintViolationSymbol.name

    override fun renderFromForStr() {
        writer.withModule(
            constraintViolationSymbol.namespace.split(constraintViolationSymbol.namespaceDelimiter).last(),
        ) {
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub struct $constraintViolationName(pub(crate) #{String});
                """,
                "String" to RuntimeType.String,
            )
        }
        writer.rustBlock("impl #T<&str> for $enumName", RuntimeType.TryFrom) {
            rust("type Error = #T;", constraintViolationSymbol)
            rustBlock("fn try_from(s: &str) -> Result<Self, <Self as #T<&str>>::Error>", RuntimeType.TryFrom) {
                rustBlock("match s") {
                    sortedMembers.forEach { member ->
                        rust("${member.value.dq()} => Ok($enumName::${member.derivedName()}),")
                    }
                    rust("_ => Err(#T(s.to_owned()))", constraintViolationSymbol)
                }
            }
        }
        writer.rustTemplate(
            """
            impl #{TryFrom}<#{String}> for $enumName {
                type Error = #{UnknownVariantSymbol};
                fn try_from(s: #{String}) -> std::result::Result<Self, <Self as #{TryFrom}<String>>::Error> {
                    s.as_str().try_into()
                }
            }
            """,
            "String" to RuntimeType.String,
            "TryFrom" to RuntimeType.TryFrom,
            "UnknownVariantSymbol" to constraintViolationSymbol,
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
            "UnknownVariantSymbol" to constraintViolationSymbol,
        )
    }
}
