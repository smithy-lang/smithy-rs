/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

class VersionTest {
    @Test
    fun `parse versions json`() {
        val version =
            Version.parse(
                """
                {
                  "gitHash": "30205973b951256c4c37b998e7a6e94fee2f6ecc",
                  "runtimeCrates": {
                    "aws-smithy-http-server": "0.60.1",
                    "aws-smithy-runtime-api": "1.1.1",
                    "aws-smithy-protocol-test": "0.60.1",
                    "aws-smithy-eventstream": "0.60.1",
                    "aws-smithy-async": "1.1.1",
                    "aws-smithy-http-server-python": "0.60.1",
                    "aws-smithy-types": "1.1.1",
                    "aws-smithy-types-convert": "0.60.1",
                    "aws-smithy-http-auth": "0.60.1",
                    "aws-smithy-checksums": "0.60.1",
                    "aws-smithy-runtime": "1.1.1",
                    "aws-smithy-query": "0.60.1",
                    "aws-smithy-xml": "0.60.1",
                    "aws-smithy-json": "0.60.1",
                    "aws-smithy-http-tower": "0.60.1",
                    "aws-smithy-http": "0.60.1",
                    "aws-smithy-client": "0.60.1",
                    "aws-sig-auth": "0.60.1",
                    "aws-credential-types": "1.1.1",
                    "aws-runtime-api": "1.1.1",
                    "aws-types": "1.1.1",
                    "aws-sigv4": "1.1.1",
                    "aws-runtime": "1.1.1",
                    "aws-http": "0.60.1",
                    "aws-endpoint": "0.60.1",
                    "aws-config": "1.1.1",
                    "aws-hyper": "0.60.1"  }
                }
                """,
            )
        version.gitHash shouldBe "30205973b951256c4c37b998e7a6e94fee2f6ecc"
        version.crates["aws-smithy-http-server"] shouldBe "0.60.1"
        version.crates["aws-config"] shouldBe "1.1.1"
    }
}
