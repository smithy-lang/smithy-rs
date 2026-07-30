/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

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
        val protocols =
            listOf(
                TestProtocol(
                    "one",
                    1,
                    ServerProtocolModules(
                        RustModule.pubCrate("protocol_serde_one"),
                        RustModule.private("event_stream_serde_one"),
                    ),
                ),
                TestProtocol(
                    "two",
                    2,
                    ServerProtocolModules(
                        RustModule.pubCrate("protocol_serde_two"),
                        RustModule.private("event_stream_serde_two"),
                    ),
                ),
            )
        val renderer = ProtocolScopedRenderer(rustCrate, protocols, TestProtocol::modules, debugMode = false)

        renderer.renderEach(operationModule) { scope ->
            val shapeModule = RustModule.pubCrate("shape_test", parent = ProtocolFunctions.serDeModule)
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
            rustTemplate(
                "pub(crate) fn ${scope.protocol.suffix}_value() -> u8 { #{outer}() }",
                "outer" to outer,
            )
        }
        rustCrate.lib {
            unitTest("both_protocol_implementations_survive") {
                rust(
                    """
                    assert_eq!(1, crate::operation::one_value());
                    assert_eq!(2, crate::operation::two_value());
                    """,
                )
            }
        }

        rustCrate.compileAndTest()
    }
}
