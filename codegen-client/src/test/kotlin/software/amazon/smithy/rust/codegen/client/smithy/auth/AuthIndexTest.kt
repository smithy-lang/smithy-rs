/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpApiKeyAuthTrait
import software.amazon.smithy.model.traits.HttpBasicAuthTrait
import software.amazon.smithy.model.traits.HttpBearerAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.util.logging.Logger

class AuthIndexTest {
    val logger = Logger.getLogger(javaClass.name)
    private val model =
        """
        namespace com.test

        use aws.auth#unsignedPayload

        @httpBearerAuth
        @httpApiKeyAuth(name: "X-Api-Key", in: "header")
        @httpBasicAuth
        @auth([httpApiKeyAuth])
        service Test {
            version: "1.0.0",
            operations: [
                GetFooServiceDefault,
                GetFooOpOverride,
                GetFooAnonymous,
                GetFooOptionalAuth,
                GetFooUnsigned
            ]
        }

        operation GetFooServiceDefault {}

        @auth([httpBasicAuth, httpBearerAuth])
        operation GetFooOpOverride{}

        @auth([])
        operation GetFooAnonymous{}

        @optionalAuth
        operation GetFooOptionalAuth{}

        @unsignedPayload
        operation GetFooUnsigned{}
        """.asSmithyModel(smithyVersion = "2.0")

    private val testCodegenContext =
        testClientCodegenContext(
            model,
            rootDecorator =
                CombinedClientCodegenDecorator(
                    listOf(
                        NoAuthDecorator(),
                        HttpAuthDecorator(),
                    ),
                ),
        )

    @Test
    fun testEffectiveOperationHandlers() {
        val testCases =
            listOf(
                "com.test#GetFooServiceDefault" to listOf(HttpApiKeyAuthTrait.ID),
                "com.test#GetFooOpOverride" to listOf(HttpBasicAuthTrait.ID, HttpBearerAuthTrait.ID),
                "com.test#GetFooAnonymous" to listOf(NoAuthSchemeOption().shapeId),
                "com.test#GetFooOptionalAuth" to listOf(NoAuthSchemeOption().shapeId),
            )
        val sut = AuthIndex(testCodegenContext)
        testCases.forEach { (opShapeId, expectedSchemes) ->
            val op = model.expectShape(ShapeId.from(opShapeId), OperationShape::class.java)
            val handlers = sut.effectiveAuthOptionsForOperation(op)
            val actualSchemes = handlers.map { it.shapeId }
            assertEquals(expectedSchemes, actualSchemes)
        }
    }

    @Test
    fun testOperationsWithOverrides() {
        val sut = AuthIndex(testCodegenContext)
        val actual = sut.operationsWithOverrides()
        val expected =
            setOf(
                model.expectShape(ShapeId.from("com.test#GetFooOpOverride"), OperationShape::class.java),
                model.expectShape(ShapeId.from("com.test#GetFooAnonymous"), OperationShape::class.java),
                model.expectShape(ShapeId.from("com.test#GetFooOptionalAuth"), OperationShape::class.java),
                model.expectShape(ShapeId.from("com.test#GetFooUnsigned"), OperationShape::class.java),
            )
        assertEquals(expected, actual)
    }
}
