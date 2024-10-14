/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import io.kotest.matchers.booleans.shouldBeTrue
import io.kotest.matchers.paths.shouldExist
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.generated.BuildEnvironment
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import java.io.File
import java.nio.file.Files.createTempDirectory
import java.util.regex.Pattern

internal class RustToolChainTomlTest {
    val model =
        """
        namespace test

        service TestService {
            version: "123",
            operations: [TestOperation]
        }

        operation TestOperation {
            input:= {}
            output:= {}
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `override test directory in integration test has a rust-toolchain toml file`() {
        val dir = createTempDirectory("smithy-test").toFile()
        val (_, path) = generatePluginContext(model, overrideTestDir = dir)
        path.shouldExist()
        val rustToolchainTomlPath = path.resolve("rust-toolchain.toml")
        rustToolchainTomlPath.shouldExist()
    }

    @Test
    fun `rust-toolchain toml file has correct value from gradle properties for rust-msrv`() {
        val (_, path) = generatePluginContext(model)
        val rustToolchainTomlPath = path.resolve("rust-toolchain.toml")
        rustToolchainTomlPath.shouldExist()

        // Read the MSRV written in `gradle.properties` file.
        val msrvPattern = Pattern.compile("rust\\.msrv=(.+)")
        val gradlePropertiesPath = File(BuildEnvironment.PROJECT_DIR).resolve("gradle.properties")
        val msrv =
            gradlePropertiesPath.useLines { lines ->
                lines.firstNotNullOfOrNull { line ->
                    msrvPattern.matcher(line).let { matcher ->
                        if (matcher.find()) matcher.group(1) else null
                    }
                }
            }
        msrv shouldNotBe null

        // Read `channel = (\d+)` from `rust-toolchain.toml` file, and
        // ensure it matches the one in `gradle.properties`.
        val toolchainPattern = Pattern.compile("\\[toolchain]")
        val channelPattern = Pattern.compile("channel\\s*=\\s*\"(.+)\"")

        val channelMatches =
            rustToolchainTomlPath.toFile().useLines { lines ->
                // Skip lines until the [toolchain] table is found, then take all lines until the next table.
                val toolchainSection =
                    lines
                        .dropWhile { !toolchainPattern.matcher(it).find() }
                        .drop(1)
                        .takeWhile { !it.trim().startsWith("[") }

                // There should be a [toolchain] table, and it must have a key called 'channel' whose value must
                // match the `rust.msrv` specified in gradle.properties.
                toolchainSection.any { line ->
                    channelPattern.matcher(line).let { matcher ->
                        matcher.find() && matcher.group(1) == msrv
                    }
                }
            }

        channelMatches.shouldBeTrue()
    }
}
