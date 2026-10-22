/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

/** Header and body assertions using the test helpers selected for the server's HTTP version. */
internal class ServerProtocolTestAssertions(private val protocolTestHelpers: RuntimeType) {
    fun checkRequiredHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        requireHeaders: List<String>,
    ) {
        basicCheck(
            requireHeaders,
            rustWriter,
            "required_headers",
            actualExpression,
            "require_headers",
        )
    }

    fun checkForbidHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        forbidHeaders: List<String>,
    ) {
        basicCheck(
            forbidHeaders,
            rustWriter,
            "forbidden_headers",
            actualExpression,
            "forbid_headers",
        )
    }

    fun checkHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        headers: Map<String, String>,
    ) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = [", "];") {
            writeWithNoFormatting(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                },
            )
        }
        assertOk(rustWriter) {
            write(
                "#T($actualExpression, $variableName)",
                protocolTestHelpers.resolve("validate_headers"),
            )
        }
    }

    fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        expectedVariableName: String,
        actualExpression: String,
        checkFunction: String,
    ) {
        if (params.isEmpty()) {
            return
        }
        rustWriter.withBlock("let $expectedVariableName = ", ";") {
            strSlice(this, params)
        }
        assertOk(rustWriter) {
            rustWriter.rust(
                "#T($actualExpression, $expectedVariableName)",
                protocolTestHelpers.resolve(checkFunction),
            )
        }
    }

    /**
     * Wraps `inner` in a call to `aws_smithy_protocol_test::assert_ok`, a convenience wrapper
     * for pretty printing protocol test helper results.
     */
    fun assertOk(
        rustWriter: RustWriter,
        inner: Writable,
    ) {
        rustWriter.rust("#T(", protocolTestHelpers.resolve("assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(
        writer: RustWriter,
        args: List<String>,
    ) {
        writer.withBlock("&[", "]") {
            rust(args.joinToString(",") { it.dq() })
        }
    }
}
