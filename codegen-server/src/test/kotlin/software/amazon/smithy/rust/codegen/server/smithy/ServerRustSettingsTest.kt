/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

internal class ServerRustSettingsTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test

        service TestService {
            version: "1"
        }
        """.asSmithyModel()

    private fun settings(codegenSettings: String): ServerRustSettings =
        ServerRustSettings.from(
            model,
            Node.parse(
                """
                {
                    "service": "test#TestService",
                    "module": "test-service",
                    "moduleVersion": "1.0.0",
                    "moduleAuthors": ["test@example.com"],
                    "codegen": {
                        $codegenSettings
                    }
                }
                """,
            ).expectObjectNode(),
        )

    @Test
    fun `schemaSerde defaults to disabled`() {
        settings("").codegenConfig.schemaSerde shouldBe false
    }

    @Test
    fun `schemaSerde can be enabled from codegen config`() {
        settings(""""schemaSerde": true""").codegenConfig.schemaSerde shouldBe true
    }
}
