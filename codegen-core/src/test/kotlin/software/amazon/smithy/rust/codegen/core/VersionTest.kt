/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core

import io.kotest.assertions.throwables.shouldThrowAny
import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource

class VersionTest {
    @ParameterizedTest()
    @MethodSource("versionProvider")
    fun `parses version`(
        content: String,
        fullVersion: String,
        crateVersion: String,
    ) {
        val version = Version.parse(content)
        version.fullVersion shouldBe fullVersion
        version.stableCrateVersion shouldBe crateVersion
    }

    @ParameterizedTest()
    @MethodSource("invalidVersionProvider")
    fun `fails to parse version`(
        content: String,
    ) {
        shouldThrowAny { Version.parse(content) }
    }

    companion object {
        @JvmStatic
        fun versionProvider() = listOf(
            Arguments.of(
                """{ "stableVersion": "0.47.0", "githash": "0198d26096eb1af510ce24766c921ffc5e4c191e" }""",
                "0.47.0-0198d26096eb1af510ce24766c921ffc5e4c191e",
                "0.47.0",
            ),
            Arguments.of(
                """{ "stableVersion": "release-2022-08-04", "githash": "db48039065bec890ef387385773b37154b555b14" }""",
                "release-2022-08-04-db48039065bec890ef387385773b37154b555b14",
                "release-2022-08-04",
            ),
        )

        @JvmStatic
        fun invalidVersionProvider() = listOf("0.0.0", "")
    }
}
