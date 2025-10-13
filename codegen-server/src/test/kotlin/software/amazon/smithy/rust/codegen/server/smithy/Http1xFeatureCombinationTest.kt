/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/**
 * Tests for http-1x flag combined with other features.
 * Phase 4: Feature Interaction - verifies http-1x works correctly with other codegen features.
 */
internal class Http1xFeatureCombinationTest {
    private fun buildAdditionalSettings(
        http1x: Boolean,
        publicConstrainedTypes: Boolean = true,
        ignoreUnsupportedConstraints: Boolean = false,
    ): ObjectNode {
        val codegenBuilder =
            Node.objectNodeBuilder()
                .withMember("http-1x", http1x)
                .withMember("publicConstrainedTypes", publicConstrainedTypes)

        if (ignoreUnsupportedConstraints) {
            codegenBuilder.withMember("ignoreUnsupportedConstraints", ignoreUnsupportedConstraints)
        }

        return Node.objectNodeBuilder()
            .withMember("codegen", codegenBuilder.build())
            .build()
    }

    @Test
    fun `http-1x works with constraints model and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `http-1x works with constraints model and http-1x enabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `http-1x works with rpcv2Cbor and constraints with http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.Rpcv2Cbor)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `http-1x works with rpcv2Cbor and constraints with http-1x enabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.Rpcv2Cbor)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `http-1x works with event streams model and http-1x disabled`() {
        val model = File("../codegen-core/common-test-models/pokemon.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `http-1x works with event streams model and http-1x enabled`() {
        val model = File("../codegen-core/common-test-models/pokemon.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `http-1x works with publicConstrainedTypes disabled and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings =
                    buildAdditionalSettings(
                        http1x = false,
                        publicConstrainedTypes = false,
                    ),
            ),
        )
    }

    @Test
    fun `http-1x works with publicConstrainedTypes disabled and http-1x enabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings =
                    buildAdditionalSettings(
                        http1x = true,
                        publicConstrainedTypes = false,
                    ),
            ),
        )
    }

    @Test
    fun `http-1x works with addValidationExceptionToConstrainedOperations and http-1x disabled`() {
        val testModel =
            """
            namespace test

            use smithy.framework#ValidationException
            use aws.protocols#restJson1

            @restJson1
            service ConstrainedService {
                operations: [SampleOperation]
            }

            @http(uri: "/sample", method: "POST")
            operation SampleOperation {
                output: SampleInputOutput
                input: SampleInputOutput
                errors: []
            }

            structure SampleInputOutput {
                constrainedInteger : RangedInteger
                @range(min: 2, max:100)
                constrainedMemberInteger : RangedInteger
                patternString : PatternString
            }

            @pattern("^[a-m]+${'$'}")
            string PatternString

            @range(min: 0, max:1000)
            integer RangedInteger
            """.asSmithyModel(smithyVersion = "2")

        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .addValidationExceptionToConstrainedOperations()
                        .withHttp1x(false)
                        .toObjectNode(),
            ),
        )
    }

    @Test
    fun `http-1x works with addValidationExceptionToConstrainedOperations and http-1x enabled`() {
        val testModel =
            """
            namespace test

            use smithy.framework#ValidationException
            use aws.protocols#restJson1

            @restJson1
            service ConstrainedService {
                operations: [SampleOperation]
            }

            @http(uri: "/sample", method: "POST")
            operation SampleOperation {
                output: SampleInputOutput
                input: SampleInputOutput
                errors: []
            }

            structure SampleInputOutput {
                constrainedInteger : RangedInteger
                @range(min: 2, max:100)
                constrainedMemberInteger : RangedInteger
                patternString : PatternString
            }

            @pattern("^[a-m]+${'$'}")
            string PatternString

            @range(min: 0, max:1000)
            integer RangedInteger
            """.asSmithyModel(smithyVersion = "2")

        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .addValidationExceptionToConstrainedOperations()
                        .withHttp1x(true)
                        .toObjectNode(),
            ),
        )
    }

    @Test
    fun `http-1x works with complex model with HTTP binding traits and http-1x disabled`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

            @restJson1
            service ComplexService {
                version: "2024-03-18"
                operations: [ComplexOperation]
            }

            @http(uri: "/complex/{pathParam}", method: "POST")
            operation ComplexOperation {
                input: ComplexInput
                output: ComplexOutput
            }

            structure ComplexInput {
                @httpLabel
                @required
                pathParam: String

                @httpQuery("query")
                queryParam: String

                @httpHeader("X-Custom-Header")
                headerParam: String

                @httpPayload
                payload: Payload
            }

            structure ComplexOutput {
                @httpHeader("X-Response-Header")
                responseHeader: String

                @httpPayload
                payload: Payload
            }

            structure Payload {
                @required
                data: String

                @length(min: 1, max: 100)
                metadata: String
            }
            """.asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `http-1x works with complex model with HTTP binding traits and http-1x enabled`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

            @restJson1
            service ComplexService {
                version: "2024-03-18"
                operations: [ComplexOperation]
            }

            @http(uri: "/complex/{pathParam}", method: "POST")
            operation ComplexOperation {
                input: ComplexInput
                output: ComplexOutput
            }

            structure ComplexInput {
                @httpLabel
                @required
                pathParam: String

                @httpQuery("query")
                queryParam: String

                @httpHeader("X-Custom-Header")
                headerParam: String

                @httpPayload
                payload: Payload
            }

            structure ComplexOutput {
                @httpHeader("X-Response-Header")
                responseHeader: String

                @httpPayload
                payload: Payload
            }

            structure Payload {
                @required
                data: String

                @length(min: 1, max: 100)
                metadata: String
            }
            """.asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `http-1x works with naming obstacle course model and http-1x disabled`() {
        val model = File("../codegen-core/common-test-models/naming-obstacle-course-ops.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false, ignoreUnsupportedConstraints = true),
            ),
        )
    }

    @Test
    fun `http-1x works with naming obstacle course model and http-1x enabled`() {
        val model = File("../codegen-core/common-test-models/naming-obstacle-course-ops.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true, ignoreUnsupportedConstraints = true),
            ),
        )
    }
}
