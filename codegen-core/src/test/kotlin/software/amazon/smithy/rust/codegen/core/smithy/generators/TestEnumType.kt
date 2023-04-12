/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

object TestEnumType : EnumType() {
    override fun implFromForStr(context: EnumGeneratorContext): Writable = writable {
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
            "matchArms" to writable {
                context.sortedMembers.forEach { member ->
                    rust("${member.value.dq()} => ${context.enumName}::${member.derivedName()},")
                }
                rust("_ => panic!()")
            },
        )
    }

    override fun implFromStr(context: EnumGeneratorContext): Writable = writable {
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
}
