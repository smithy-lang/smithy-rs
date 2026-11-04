/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.http

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

/**
 * Renders the client policy decision for a failed modeled response-header parse.
 *
 * Parsing must fail before callers evaluate this expression. A readable malformed value therefore
 * remains an error under `Skip`, while any unreadable value applies `Skip` to the whole member.
 * Server request parsing deliberately does not call this helper and remains strict.
 */
fun nonUtf8HeaderShouldBeSkipped(
    runtimeConfig: RuntimeConfig,
    configBagExpression: String,
    unreadableScan: Writable,
): Writable =
    writable {
        rustTemplate(
            """
            {
                let has_unreadable_value = #{UnreadableScan:W};
                has_unreadable_value
                    && $configBagExpression.load::<#{NonUtf8HeaderHandling}>()
                        == #{Some}(&#{NonUtf8HeaderHandling}::Skip)
            }
            """,
            *preludeScope,
            "UnreadableScan" to unreadableScan,
            "NonUtf8HeaderHandling" to
                RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::NonUtf8HeaderHandling"),
        )
    }
