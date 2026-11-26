/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import kotlin.io.path.readText

/**
 * The HTTP-client features of a generated SDK crate.
 *
 * `LegacyClientTest` asserts the same kind of thing for generic Smithy clients, but it exercises
 * `FluentClientDecorator`, which the SDK does not use. Without this, pointing `rustls` back at
 * `tls-rustls` or making it a default again would put `http` 0.2.x back in every default build with
 * nothing to catch it.
 */
class AwsFluentClientFeaturesTest {
    @Test
    fun `default features select only the hyper 1x client`() {
        val testDir =
            awsSdkIntegrationTest(
                SdkCodegenIntegrationTest.model,
                params = awsIntegrationTestParams().copy(cargoCommand = "cargo check"),
            )
        val lines = testDir.resolve("Cargo.toml").readText().lines()

        fun feature(name: String): String =
            lines.singleOrNull { it.startsWith("$name = ") }
                ?: error("Expected exactly one `$name` feature:\n${lines.joinToString("\n")}")

        val default = feature("default")
        assert(default.contains("\"default-https-client\"")) {
            "`default-https-client` must be a default feature: $default"
        }
        assert(!default.contains("\"rustls\"")) { "`rustls` must not be a default feature: $default" }
        assert(!default.contains("\"legacy-https-client\"")) {
            "`legacy-https-client` must not be a default feature: $default"
        }

        // `rustls` stays accepted as a name, but now resolves to the hyper 1.x client. Only
        // `legacy-https-client` reaches the hyper 0.14.x stack, and nothing enables it implicitly.
        assertEquals("default-https-client = [\"aws-smithy-runtime/default-https-client\"]", feature("default-https-client"))
        assertEquals("rustls = [\"default-https-client\"]", feature("rustls"))
        assertEquals("legacy-https-client = [\"aws-smithy-runtime/tls-rustls\"]", feature("legacy-https-client"))
    }
}
