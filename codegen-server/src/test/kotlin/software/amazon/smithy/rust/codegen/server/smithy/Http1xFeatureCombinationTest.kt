/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.EnumSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.loadSmithyConstraintsModelForProtocol
import software.amazon.smithy.rust.codegen.server.smithy.ModelProtocol
import java.io.File
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.Model

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

    private fun testHttp1xWithModel(
        model: Model,
        serviceShapeId: ShapeId? = null,
        http1x: Boolean,
        publicConstrainedTypes: Boolean = true,
        ignoreUnsupportedConstraints: Boolean = false,
        addValidationExceptionToConstrainedOperations: Boolean = false,
    ) {
        val builder = ServerAdditionalSettings.builder()
            .withHttp1x(http1x)
            .publicConstrainedTypes(publicConstrainedTypes)
           .ignoreUnsupportedConstraints(ignoreUnsupportedConstraints)
           
        if (addValidationExceptionToConstrainedOperations) {
            builder.addValidationExceptionToConstrainedOperations()
        }

        val params = IntegrationTestParams(
            additionalSettings = builder.toObjectNode(),
            command = {}
        )
        
        val finalParams = if (serviceShapeId != null) {
            params.copy(service = serviceShapeId.toString())
        } else {
            params
        }
        
        serverIntegrationTest(model, finalParams)
    }

    private fun loadPokemonModel(): Model =
        File("../codegen-core/common-test-models/pokemon.smithy").readText().asSmithyModel()

    private fun loadHttpQueryModel(): Model =
        """
        namespace test

        use smithy.framework#ValidationException
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
            errors: [ValidationException]
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
            
            constrainedInteger: RangedInteger
            
            @range(min: 2, max: 100)
            constrainedMemberInteger: RangedInteger
            
            patternString: PatternString
        }

        @pattern("^[a-m]+$")
        string PatternString

        @range(min: 0, max: 1000)
        integer RangedInteger
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `constraints model with AwsJson10 and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.AwsJson10)
        testHttp1xWithModel(model, serviceShapeId, http1x = false)
    }

    @Test
    fun `constraints model with AwsJson11 and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.AwsJson11)
        testHttp1xWithModel(model, serviceShapeId, http1x = false)
    }

    @Test
    fun `constraints model with RestJson and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        testHttp1xWithModel(model, serviceShapeId, http1x = false)
    }

    @Test
    fun `constraints model with RestXml and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestXml)
        testHttp1xWithModel(model, serviceShapeId, http1x = false)
    }

    @Test
    fun `constraints model with Rpcv2Cbor and http-1x disabled`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.Rpcv2Cbor)
        testHttp1xWithModel(model, serviceShapeId, http1x = false)
    }

    @ParameterizedTest
    @EnumSource(ModelProtocol::class)
    fun `constraints model with http-1x enabled`(protocol: ModelProtocol) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(protocol)
        testHttp1xWithModel(model, serviceShapeId, http1x = true)
    }

    @ParameterizedTest
    @EnumSource(ModelProtocol::class)
    fun `publicConstrainedTypes disabled with http-1x disabled`(protocol: ModelProtocol) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(protocol)
        testHttp1xWithModel(model, serviceShapeId, http1x = false, publicConstrainedTypes = false)
    }

    @ParameterizedTest
    @EnumSource(ModelProtocol::class)
    fun `publicConstrainedTypes disabled with http-1x enabled`(protocol: ModelProtocol) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(protocol)
        testHttp1xWithModel(model, serviceShapeId, http1x = true, publicConstrainedTypes = false)
    }

    @Test
    fun `httpQuery binding traits and constraints with http-1x disabled`() {
        val model = loadHttpQueryModel()
        testHttp1xWithModel(model, http1x = false)
    }

    @Test
    fun `httpQuery binding traits and constraints with http-1x enabled`() {
        val model = loadHttpQueryModel()
        testHttp1xWithModel(model, http1x = true)
    }

    @Test
    fun `httpQuery binding traits and ValidationException with http-1x disabled`() {
        val model = loadHttpQueryModel()
        testHttp1xWithModel(model, http1x = false, addValidationExceptionToConstrainedOperations = true)
    }

    @Test
    fun `httpQuery binding traits and ValidationException with http-1x enabled`() {
        val model = loadHttpQueryModel()
        testHttp1xWithModel(model, http1x = true, addValidationExceptionToConstrainedOperations = true)
    }
}
