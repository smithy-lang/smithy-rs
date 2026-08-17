/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class AllowMissingUnionVariantTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test
        use aws.protocols#restJson1
        use smithy.framework#ValidationException
        use smithy.test#httpRequestTests

        @restJson1
        service AllowMissingUnionVariantService {
            version: "0.1",
            operations: [UnionWithMissingVariantOperation],
        }

        @http(uri: "/union-with-missing-variant", method: "POST")
        @httpRequestTests([
            {
                id: "UnionWithEmptyBody",
                uri: "/union-with-missing-variant",
                method: "POST",
                protocol: "aws.protocols#restJson1",
                body: "{\"member\": {}}",
                headers: { "Content-Type": "application/json" },
                params: { },
                appliesTo: "server",
            }
        ])
        operation UnionWithMissingVariantOperation {
            input: UnionWithMissingVariantInput,
            errors: [ValidationException],
        }

        structure UnionWithMissingVariantInput {
            member: UnionWithMissingVariantUnion,
        }

        union UnionWithMissingVariantUnion {
            variant: String,
        }
        """.asSmithyModel()

    private fun runWithAllowMissingUnionVariant(enabled: Boolean) =
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "test#AllowMissingUnionVariantService",
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .allowMissingUnionVariant(enabled)
                        .toObjectNode(),
            ),
            testCoverage = HttpTestType.Default,
        ) { _, _ -> }

    @Test
    fun `an empty union body parses to None when allowMissingUnionVariant is enabled`() {
        runWithAllowMissingUnionVariant(enabled = true)
    }

    @Test
    fun `an empty union body fails the httpRequestTest when allowMissingUnionVariant is disabled`() {
        val error =
            assertThrows<CommandError> {
                runWithAllowMissingUnionVariant(enabled = false)
            }
        error.message shouldContain "Union did not contain a valid variant."
    }
}
