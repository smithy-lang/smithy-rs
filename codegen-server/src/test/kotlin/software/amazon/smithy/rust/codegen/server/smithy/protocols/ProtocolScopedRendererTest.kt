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
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.CommandError

class ProtocolScopedRendererTest {
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
                serde = RustModule.private("serde", parent = root),
                eventStreamSerde = RustModule.private("event_stream_serde", parent = root),
            )
        }
        val protocols =
            listOf(
                TestProtocol("one", 1, modules("one")),
                TestProtocol("two", 2, modules("two")),
            )
        val renderer = ProtocolScopedRenderer(rustCrate, protocols, TestProtocol::modules, debugMode = false)

        renderer.renderEach({ it.modules.operations }) { scope ->
            val shapeModule = RustModule.pubCrate("shape_test", parent = ProtocolFunctions.defaultSerDeModule)
            val nested =
                RuntimeType.forInlineFun("nested", shapeModule) {
                    rust("pub(crate) fn nested() -> u8 { ${scope.protocol.value} }")
                }
            val outer =
                RuntimeType.forInlineFun("outer", shapeModule) {
                    rustTemplate(
                        "pub(crate) fn outer() -> u8 { #{nested}() }",
                        "nested" to nested,
                    )
                }
            val functionName = "${scope.protocol.suffix}_value"
            rustTemplate(
                "pub(crate) fn $functionName() -> u8 { #{outer}() }",
                "outer" to outer,
            )
            unitTest("${scope.protocol.suffix}_implementation_survives") {
                rust("assert_eq!(${scope.protocol.value}, $functionName());")
            }
        }

        rustCrate.compileAndTest()
    }

    @Test
    fun `private protocol modules reject cross protocol serde calls`() {
        val rustCrate = TestWorkspace.testProject()
        val protocolOne = RustModule.private("protocol_one")
        val protocolTwo = RustModule.private("protocol_two")
        val protocolOneSerde = RustModule.private("serde", parent = protocolOne)
        val protocolTwoOperations = RustModule.private("operations", parent = protocolTwo)

        rustCrate.withModule(protocolOneSerde) {
            rust("pub(crate) fn secret() {}")
        }
        rustCrate.withModule(protocolTwoOperations) {
            rust("fn invalid_cross_protocol_call() { crate::protocol_one::serde::secret(); }")
        }

        val error =
            assertThrows<CommandError> {
                rustCrate.compileAndTest(expectFailure = true)
            }
        error.message shouldContain "module `serde` is private"
    }
}
