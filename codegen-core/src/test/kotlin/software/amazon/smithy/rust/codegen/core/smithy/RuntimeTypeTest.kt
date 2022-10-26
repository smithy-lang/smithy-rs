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
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyLocation
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

    @ParameterizedTest
    @MethodSource("runtimeCrateLocationProvider")
    fun `runtimeCrateLocation provides dependency location`(
        path: String?,
        versions: CrateVersionMap,
        crateName: String?,
        expectedDependencyLocation: DependencyLocation,
    ) {
        val crateLoc = RuntimeCrateLocation(path, versions)
        val depLoc = crateLoc.crateLocation(crateName)
        depLoc shouldBe expectedDependencyLocation
    }

    companion object {
        @JvmStatic
        private val defaultVersion = defaultRuntimeCrateVersion()

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

        @JvmStatic
        fun runtimeCrateLocationProvider() = listOf(
            // If user specifies `relativePath` in `runtimeConfig`, then that always takes precedence over versions.
            Arguments.of(
                "/path",
                mapOf<String, String>(),
                null,
                Local("/path"),
            ),
            Arguments.of(
                "/path",
                mapOf("a" to "1.0", "b" to "2.0"),
                null,
                Local("/path"),
            ),
            Arguments.of(
                "/path",
                mapOf("DEFAULT" to "0.1", "a" to "1.0", "b" to "2.0"),
                null,
                Local("/path", "0.1"),
            ),

            // User does not specify the versions object.
            // The version number of the code-generator should be used as the version for all runtime crates.
            Arguments.of(
                null,
                mapOf<String, String>(),
                null,
                CratesIo(defaultVersion),
            ),
            Arguments.of(
                null,
                mapOf<String, String>(),
                "a",
                CratesIo(defaultVersion),
            ),

            // User specifies versions object, setting explicit version numbers for some runtime crates.
            // Then the rest of the runtime crates use the code-generator's version as their version.
            Arguments.of(
                null,
                mapOf("a" to "1.0", "b" to "2.0"),
                null,
                CratesIo(defaultVersion),
            ),
            Arguments.of(
                null,
                mapOf("a" to "1.0", "b" to "2.0"),
                "a",
                CratesIo("1.0"),
            ),
            Arguments.of(
                null,
                mapOf("a" to "1.0", "b" to "2.0"),
                "b",
                CratesIo("2.0"),
            ),
            Arguments.of(
                null,
                mapOf("a" to "1.0", "b" to "2.0"),
                "c",
                CratesIo(defaultVersion),
            ),

            // User specifies versions object, setting DEFAULT and setting version numbers for some runtime crates.
            // Then the specified version in DEFAULT is used for all runtime crates, except for those where the user specified a value for in the map.
            Arguments.of(
                null,
                mapOf("DEFAULT" to "0.1", "a" to "1.0", "b" to "2.0"),
                null,
                CratesIo("0.1"),
            ),
            Arguments.of(
                null,
                mapOf("DEFAULT" to "0.1", "a" to "1.0", "b" to "2.0"),
                "a",
                CratesIo("1.0"),
            ),
            Arguments.of(
                null,
                mapOf("DEFAULT" to "0.1", "a" to "1.0", "b" to "2.0"),
                "b",
                CratesIo("2.0"),
            ),
            Arguments.of(
                null,
                mapOf("DEFAULT" to "0.1", "a" to "1.0", "b" to "2.0"),
                "c",
                CratesIo("0.1"),
            ),
        )
    }
}
