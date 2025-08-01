/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpApiKeyAuthTrait
import software.amazon.smithy.model.traits.HttpBasicAuthTrait
import software.amazon.smithy.model.traits.HttpBearerAuthTrait
import software.amazon.smithy.model.traits.synthetic.NoAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class AuthIndexTest {
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

    private fun testCodegenWithModel(model: Model) =
        testClientCodegenContext(
            model,
            rootDecorator =
                CombinedClientCodegenDecorator(
                    listOf(
                        AuthDecorator(),
                        NoAuthDecorator(),
                        HttpAuthDecorator(),
                    ),
                ),
        )

    private val testCodegenContext = testCodegenWithModel(model)

    @Test
    fun testAuthOptionsDedup() {
        class TestAuthSchemeOption(
            override val authSchemeId: ShapeId,
            val testId: String,
        ) : AuthSchemeOption {
            override fun render(
                codegenContext: ClientCodegenContext,
                operation: OperationShape?,
            ) = writable {}
        }

        val testCtx =
            testClientCodegenContext(
                model,
                rootDecorator =
                    CombinedClientCodegenDecorator(
                        listOf(
                            object : ClientCodegenDecorator {
                                override val name = "TestAuthDecorator1"
                                override val order: Byte = 0

                                override fun authSchemeOptions(
                                    codegenContext: ClientCodegenContext,
                                    baseAuthSchemeOptions: List<AuthSchemeOption>,
                                ) = listOf(TestAuthSchemeOption(HttpApiKeyAuthTrait.ID, "test-id-1"))
                            },
                            object : ClientCodegenDecorator {
                                override val name = "TestAuthDecorator2"
                                override val order: Byte = 10

                                override fun authSchemeOptions(
                                    codegenContext: ClientCodegenContext,
                                    baseAuthSchemeOptions: List<AuthSchemeOption>,
                                ) = listOf(TestAuthSchemeOption(HttpApiKeyAuthTrait.ID, "test-id-2"))
                            },
                        ),
                    ),
            )
        val sut = AuthIndex(testCtx)
        val authOptions = sut.authOptions().values.toList()
        assertEquals(1, authOptions.size)
        assertEquals("test-id-1", (authOptions[0] as TestAuthSchemeOption).testId)
    }

    @Test
    fun testEffectiveServiceAuthOptions() {
        val sut = AuthIndex(testCodegenContext)
        val effectiveServiceAuthOptions = sut.effectiveAuthOptionsForService()

        val actual = effectiveServiceAuthOptions.map { it.authSchemeId }
        val expected = listOf(HttpApiKeyAuthTrait.ID)
        assertEquals(expected, actual)
    }

    @Test
    fun testEffectiveServiceOptionsWithNoAuth() {
        val sut =
            AuthIndex(
                testCodegenWithModel(
                    """
                    namespace com.test

                    use aws.protocols#awsJson1_1

                    @awsJson1_1
                    service Test {
                        version: "1.0.0"
                    }
                    """.asSmithyModel(smithyVersion = "2.0"),
                ),
            )
        val effectiveServiceAuthOptions = sut.effectiveAuthOptionsForService()

        val actual = effectiveServiceAuthOptions.map { it.authSchemeId }
        val expected = listOf(NoAuthTrait.ID)
        assertEquals(expected, actual)
    }

    @Test
    fun testEffectiveAuthOptionsForOperation() {
        val testCases =
            listOf(
                "com.test#GetFooServiceDefault" to listOf(HttpApiKeyAuthTrait.ID),
                "com.test#GetFooOpOverride" to listOf(HttpBasicAuthTrait.ID, HttpBearerAuthTrait.ID),
                "com.test#GetFooAnonymous" to listOf(NoAuthSchemeOption().authSchemeId),
                "com.test#GetFooOptionalAuth" to listOf(HttpApiKeyAuthTrait.ID, NoAuthSchemeOption().authSchemeId),
            )
        val sut = AuthIndex(testCodegenContext)
        testCases.forEach { (opShapeId, expectedSchemes) ->
            val op = model.expectShape(ShapeId.from(opShapeId), OperationShape::class.java)
            val handlers = sut.effectiveAuthOptionsForOperation(op)
            val actualSchemes = handlers.map { it.authSchemeId }
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
            )
        assertEquals(expected, actual)
    }
}
