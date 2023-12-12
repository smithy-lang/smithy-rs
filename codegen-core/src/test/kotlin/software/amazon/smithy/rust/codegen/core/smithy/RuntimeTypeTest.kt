/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.Local
import java.util.Optional

class RuntimeTypesTest {
    @ParameterizedTest
    @MethodSource("runtimeConfigProvider")
    fun `succeeded to parse runtime config`(
        runtimeConfig: String,
        expectedCrateLocation: RuntimeCrateLocation,
    ) {
        val node = Node.parse(runtimeConfig)
        val cfg = RuntimeConfig.fromNode(node.asObjectNode())
        cfg.runtimeCrateLocation shouldBe expectedCrateLocation
    }

    @Test
    fun `succeeded to provide a default runtime config if missing`() {
        // This default config should share the same behaviour with `{}` empty object.
        val cfg = RuntimeConfig.fromNode(Optional.empty())
        cfg.runtimeCrateLocation shouldBe RuntimeCrateLocation(null, CrateVersionMap(mapOf()))
    }

    @Test
    fun `runtimeCrateLocation provides dependency location`() {
        val crateLoc = RuntimeCrateLocation("/foo", CrateVersionMap(mapOf("aws-smithy-runtime-api" to "999.999")))
        crateLoc.crateLocation("aws-smithy-runtime") shouldBe Local("/foo", null)
        crateLoc.crateLocation("aws-smithy-runtime-api") shouldBe Local("/foo", null)
        crateLoc.crateLocation("aws-smithy-http") shouldBe Local("/foo", null)

        val crateLocVersioned = RuntimeCrateLocation(null, CrateVersionMap(mapOf("aws-smithy-runtime-api" to "999.999")))
        crateLocVersioned.crateLocation("aws-smithy-runtime") shouldBe CratesIo(Version.stableCrateVersion())
        crateLocVersioned.crateLocation("aws-smithy-runtime-api") shouldBe CratesIo("999.999")
        crateLocVersioned.crateLocation("aws-smithy-http") shouldBe CratesIo(Version.unstableCrateVersion())
    }

    companion object {

        @JvmStatic
        fun runtimeConfigProvider() = listOf(
            Arguments.of(
                "{}",
                RuntimeCrateLocation(null, CrateVersionMap(mapOf())),
            ),
            Arguments.of(
                """
                {
                    "relativePath": "/path"
                }
                """,
                RuntimeCrateLocation("/path", CrateVersionMap(mapOf())),
            ),
            Arguments.of(
                """
                {
                    "versions": {
                        "a": "1.0",
                        "b": "2.0"
                    }
                }
                """,
                RuntimeCrateLocation(null, CrateVersionMap(mapOf("a" to "1.0", "b" to "2.0"))),
            ),
            Arguments.of(
                """
                {
                    "relativePath": "/path",
                    "versions": {
                        "a": "1.0",
                        "b": "2.0"
                    }
                }
                """,
                RuntimeCrateLocation("/path", CrateVersionMap(mapOf("a" to "1.0", "b" to "2.0"))),
            ),
        )
    }
}
