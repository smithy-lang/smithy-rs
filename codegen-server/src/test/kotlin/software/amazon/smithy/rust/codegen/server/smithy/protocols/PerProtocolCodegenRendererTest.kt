/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolCodegenModules
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.CommandError

class PerProtocolCodegenRendererTest {
    @Test
    fun `isolates colliding protocol functions including lazy nested dependencies`() {
        val rustCrate = TestWorkspace.testProject()
        val operationModule = RustModule.private("operation")

        data class TestProtocol(
            val suffix: String,
            val value: Int,
            val modules: ServerProtocolModules,
        )

        fun modules(suffix: String): ServerProtocolModules {
            val root = RustModule.private("protocol_$suffix")
            return ServerProtocolModules(
                operations = RustModule.private("operations", parent = root),
                protocolCodegenModules = ProtocolCodegenModules.under(root),
                protocolTests = RustModule.private("protocol_tests_$suffix").cfgTest(),
            )
        }
        val protocols =
            listOf(
                TestProtocol("one", 1, modules("one")),
                TestProtocol("two", 2, modules("two")),
            )
        val renderer = PerProtocolCodegenRenderer(rustCrate, protocols)

        renderer.renderEach({ it.modules.operations }) { context ->
            val shapeModule = RustModule.pubCrate("shape_test", parent = context.protocol.modules.serde)
            val nested =
                RuntimeType.forInlineFun("nested", shapeModule) {
                    rust("pub(crate) fn nested() -> u8 { ${context.protocol.value} }")
                }
            val outer =
                RuntimeType.forInlineFun("outer", shapeModule) {
                    rustTemplate(
                        "pub(crate) fn outer() -> u8 { #{nested}() }",
                        "nested" to nested,
                    )
                }
            val functionName = "${context.protocol.suffix}_value"
            rustTemplate(
                "pub(crate) fn $functionName() -> u8 { #{outer}() }",
                "outer" to outer,
            )
            unitTest("${context.protocol.suffix}_implementation_survives") {
                rust("assert_eq!(${context.protocol.value}, $functionName());")
            }
        }

        rustCrate.compileAndTest()
    }

    @Test
    fun `private protocol modules reject cross protocol serde calls`() {
        val rustCrate = TestWorkspace.testProject()
        val protocolOne = RustModule.private("protocol_one")
        val protocolTwo = RustModule.private("protocol_two")
        val protocolOneSerde = RustModule.private("protocol_serde", parent = protocolOne)
        val protocolTwoOperations = RustModule.private("operations", parent = protocolTwo)

        rustCrate.withModule(protocolOneSerde) {
            rust("pub(crate) fn secret() {}")
        }
        rustCrate.withModule(protocolTwoOperations) {
            rust("fn invalid_cross_protocol_call() { crate::protocol_one::protocol_serde::secret(); }")
        }

        val error =
            assertThrows<CommandError> {
                rustCrate.compileAndTest(expectFailure = true)
            }
        error.message shouldContain "module `protocol_serde` is private"
    }
}
