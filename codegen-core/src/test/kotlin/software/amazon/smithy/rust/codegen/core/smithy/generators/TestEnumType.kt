/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq

object TestEnumType : EnumType() {
    override fun implFromForStr(context: EnumGeneratorContext): Writable =
        writable {
            rustTemplate(
                """
                impl #{From}<&str> for ${context.enumName} {
                    fn from(s: &str) -> Self {
                        match s {
                            #{matchArms}
                        }
                    }
                }
                """,
                "From" to RuntimeType.From,
                "matchArms" to
                    writable {
                        context.sortedMembers.forEach { member ->
                            rawRust("${member.value.dq()} => ${context.enumName}::${member.derivedName()},\n")
                        }
                        rawRust("_ => panic!()")
                    },
            )
        }

    override fun implFromStr(context: EnumGeneratorContext): Writable =
        writable {
            rust(
                """
                impl std::str::FromStr for ${context.enumName} {
                    type Err = std::convert::Infallible;
                    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                        Ok(${context.enumName}::from(s))
                    }
                }
                """,
            )
        }

    override fun implFromForStrForUnnamedEnum(context: EnumGeneratorContext): Writable =
        writable {
            rustTemplate(
                """
                impl<T> #{From}<T> for ${context.enumName} where T: #{AsRef}<str> {
                    fn from(s: T) -> Self {
                        ${context.enumName}(s.as_ref().to_owned())
                    }
                }
                """,
                *preludeScope,
            )
        }

    override fun implFromStrForUnnamedEnum(context: EnumGeneratorContext): Writable =
        writable {
            // Add an infallible FromStr implementation for uniformity
            rustTemplate(
                """
                impl ::std::str::FromStr for ${context.enumName} {
                    type Err = ::std::convert::Infallible;
    
                    fn from_str(s: &str) -> #{Result}<Self, <Self as ::std::str::FromStr>::Err> {
                        #{Ok}(${context.enumName}::from(s))
                    }
                }
                """,
                *preludeScope,
            )
        }
}
