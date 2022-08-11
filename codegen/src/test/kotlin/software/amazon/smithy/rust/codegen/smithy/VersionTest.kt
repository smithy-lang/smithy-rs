/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

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
        version.crateVersion shouldBe crateVersion
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
                "0.47.0\n0198d26096eb1af510ce24766c921ffc5e4c191e",
                "0.47.0-0198d26096eb1af510ce24766c921ffc5e4c191e",
                "0.47.0",
            ),
            Arguments.of(
                "release-2022-08-04\ndb48039065bec890ef387385773b37154b555b14",
                "release-2022-08-04-db48039065bec890ef387385773b37154b555b14",
                "release-2022-08-04",
            ),
            Arguments.of(
                "0.30.0-alpha\na1dbbe2947de3c8bbbef9446eb442e298f83f200",
                "0.30.0-alpha-a1dbbe2947de3c8bbbef9446eb442e298f83f200",
                "0.30.0-alpha",
            ),
            Arguments.of(
                "0.6-rc1.cargo\nc281800a185b34600b05f8b501a0322074184123",
                "0.6-rc1.cargo-c281800a185b34600b05f8b501a0322074184123",
                "0.6-rc1.cargo",
            ),
            Arguments.of(
                "0.27.0-alpha.1\n643f2ee",
                "0.27.0-alpha.1-643f2ee",
                "0.27.0-alpha.1",
            ),
        )

        @JvmStatic
        fun invalidVersionProvider() = listOf("0.0.0", "")
    }
}
