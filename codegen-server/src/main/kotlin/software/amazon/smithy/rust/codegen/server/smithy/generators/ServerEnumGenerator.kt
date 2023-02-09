/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGeneratorContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumType
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.PubCrateConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

open class ConstrainedEnum(
    codegenContext: ServerCodegenContext,
    private val shape: StringShape,
) : EnumType() {
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
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
    )

    override fun implFromForStr(context: EnumGeneratorContext): Writable = writable {
        withInlineModule(constraintViolationSymbol.module()) {
            rustTemplate(
                """
                ##[derive(Debug, PartialEq)]
                pub struct $constraintViolationName(pub(crate) #{String});
                """,
                *codegenScope,
            )

            if (shape.isReachableFromOperationInput()) {
                val enumValueSet = context.enumTrait.enumDefinitionValues.joinToString(", ")
                val message = "Value {} at '{}' failed to satisfy constraint: Member must satisfy enum value set: [$enumValueSet]"

                rustTemplate(
                    """
                    impl $constraintViolationName {
                        pub(crate) fn as_validation_exception_field(self, path: #{String}) -> crate::model::ValidationExceptionField {
                            crate::model::ValidationExceptionField {
                                message: format!(r##"$message"##, &self.0, &path),
                                path,
                            }
                        }
                    }
                    """,
                    *codegenScope,
                )
            }
        }
        rustBlock("impl #T<&str> for ${context.enumName}", RuntimeType.TryFrom) {
            rust("type Error = #T;", constraintViolationSymbol)
            rustBlock("fn try_from(s: &str) -> Result<Self, <Self as #T<&str>>::Error>", RuntimeType.TryFrom) {
                rustBlock("match s") {
                    context.sortedMembers.forEach { member ->
                        rust("${member.value.dq()} => Ok(${context.enumName}::${member.derivedName()}),")
                    }
                    rust("_ => Err(#T(s.to_owned()))", constraintViolationSymbol)
                }
            }
        }
        rustTemplate(
            """
            impl #{TryFrom}<#{String}> for ${context.enumName} {
                type Error = #{ConstraintViolation};
                fn try_from(s: #{String}) -> std::result::Result<Self, <Self as #{TryFrom}<String>>::Error> {
                    s.as_str().try_into()
                }
            }
            """,
            "String" to RuntimeType.String,
            "TryFrom" to RuntimeType.TryFrom,
            "ConstraintViolation" to constraintViolationSymbol,
        )
    }

    override fun implFromStr(context: EnumGeneratorContext): Writable = writable {
        rustTemplate(
            """
            impl std::str::FromStr for ${context.enumName} {
                type Err = #{ConstraintViolation};
                fn from_str(s: &str) -> std::result::Result<Self, <Self as std::str::FromStr>::Err> {
                    Self::try_from(s)
                }
            }
            """,
            "ConstraintViolation" to constraintViolationSymbol,
        )
    }
}

class ServerEnumGenerator(
    codegenContext: ServerCodegenContext,
    shape: StringShape,
) : EnumGenerator(
    codegenContext.model,
    codegenContext.symbolProvider,
    shape,
    enumType = ConstrainedEnum(codegenContext, shape),
)
