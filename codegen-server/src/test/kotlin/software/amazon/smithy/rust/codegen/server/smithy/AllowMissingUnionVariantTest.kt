/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.node.ObjectNode
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
            // Every place a union can be read from, so the generated code for each compiles under the setting.
            unions: UnionList,
            sparseUnions: SparseUnionList,
            unionMap: UnionMap,
            sparseUnionMap: SparseUnionMap,
            outer: OuterUnion,
            constrained: ConstrainedUnion,
        }

        union UnionWithMissingVariantUnion {
            variant: String,
        }

        list UnionList {
            member: UnionWithMissingVariantUnion,
        }

        @sparse
        list SparseUnionList {
            member: UnionWithMissingVariantUnion,
        }

        map UnionMap {
            key: String,
            value: UnionWithMissingVariantUnion,
        }

        @sparse
        map SparseUnionMap {
            key: String,
            value: UnionWithMissingVariantUnion,
        }

        union OuterUnion {
            inner: UnionWithMissingVariantUnion,
        }

        union ConstrainedUnion {
            @length(min: 1)
            name: String,
        }
        """.asSmithyModel()

    private fun runWithAllowMissingUnionVariant(
        enabled: Boolean,
        schemaSerde: Boolean = false,
    ) = serverIntegrationTest(
        model,
        IntegrationTestParams(
            service = "test#AllowMissingUnionVariantService",
            additionalSettings = settings(enabled, schemaSerde),
        ),
        testCoverage = HttpTestType.Default,
    ) { _, _ -> }

    /** The schema-based request path needs `schemaSerde` and `http1x`; the builder has no `schemaSerde` knob. */
    private fun settings(
        allowMissingUnionVariant: Boolean,
        schemaSerde: Boolean,
    ): ObjectNode {
        val builder = ServerAdditionalSettings.builder().allowMissingUnionVariant(allowMissingUnionVariant)
        if (!schemaSerde) {
            return builder.toObjectNode()
        }
        val node = builder.withHttp1x().toObjectNode()
        val codegen =
            node.expectObjectMember("codegen").toBuilder()
                .withMember(ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY, true)
                .build()
        return node.toBuilder().withMember("codegen", codegen).build()
    }

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

    @Test
    fun `on the schema path an empty union body parses to None when allowMissingUnionVariant is enabled`() {
        runWithAllowMissingUnionVariant(enabled = true, schemaSerde = true)
    }

    @Test
    fun `on the schema path an empty union body fails the httpRequestTest when allowMissingUnionVariant is disabled`() {
        val error =
            assertThrows<CommandError> {
                runWithAllowMissingUnionVariant(enabled = false, schemaSerde = true)
            }
        // The schema path rejects the request before the handler; the generated protocol test reports that
        // rather than the deserializer's message.
        error.message shouldContain "we expected operation handler to be invoked but it was not entered"
    }
}
