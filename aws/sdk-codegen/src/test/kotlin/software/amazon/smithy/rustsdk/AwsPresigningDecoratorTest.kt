/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rustsdk.traits.PresignableTrait

class AwsPresigningDecoratorTest {
    @Test
    fun `it should add the synthetic presignable trait to known presignable operations`() {
        testTransform("some.service", "NotPresignable", presignable = false)
        testTransform("com.amazonaws.s3", "GetObject", presignable = true)
    }

    private fun testTransform(namespace: String, name: String, presignable: Boolean) {
        val decorator = AwsPresigningDecorator()
        val model = testOperation(namespace, name)
        val transformed = decorator.transformModel(serviceShape(model), model)
        hasPresignableTrait(transformed) shouldBe presignable
    }

    private fun hasPresignableTrait(model: Model): Boolean =
        model.shapes().filter { shape -> shape is OperationShape }
            .findFirst()
            .orNull()!!
            .hasTrait(PresignableTrait.ID)

    private fun serviceShape(model: Model): ServiceShape =
        model.shapes().filter { shape -> shape is ServiceShape }.findFirst().orNull()!! as ServiceShape

    private fun testOperation(namespace: String, name: String): Model =
        """
            namespace $namespace
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2019-12-16",
                operations: ["$name"],
            }

            operation $name {
                input: ${name}InputOutput,
                output: ${name}InputOutput,
            }
            structure ${name}InputOutput {
            }
        """.asSmithyModel()
}

class OverrideHttpMethodTransformTest {
    @Test
    fun `it should override the HTTP method for the listed operations`() {
        val model = """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2019-12-16",
                operations: ["One", "Two", "Three"],
            }

            structure TestInput { }

            @http(uri: "/one", method: "POST")
            operation One { input: TestInput }

            @http(uri: "/two", method: "GET")
            operation Two { input: TestInput }

            @http(uri: "/three", method: "POST")
            operation Three { input: TestInput }
        """.asSmithyModel()

        val transformed = OverrideHttpMethodTransform(
            mapOf(
                ShapeId.from("test#One") to "GET",
                ShapeId.from("test#Two") to "POST",
            )
        ).transform(model)

        transformed.expectShape(ShapeId.from("test#One")).expectTrait<HttpTrait>().method shouldBe "GET"
        transformed.expectShape(ShapeId.from("test#Two")).expectTrait<HttpTrait>().method shouldBe "POST"
        transformed.expectShape(ShapeId.from("test#Three")).expectTrait<HttpTrait>().method shouldBe "POST"
    }
}

class MoveDocumentMembersToQueryParamsTransformTest {
    @Test
    fun `it should move document members to query parameters for the listed operations`() {
        val model = """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2019-12-16",
                operations: ["One", "Two"],
            }

            structure OneInput {
                @httpHeader("one")
                one: String,
                @httpQuery("two")
                two: String,

                three: String,
                four: String,
            }
            structure TwoInput {
                @httpHeader("one")
                one: String,
                @httpQuery("two")
                two: String,

                three: String,
                four: String,
            }

            @http(uri: "/one", method: "POST")
            operation One { input: OneInput }

            @http(uri: "/two", method: "POST")
            operation Two { input: TwoInput }
        """.asSmithyModel()

        val transformed = MoveDocumentMembersToQueryParamsTransform(
            listOf(ShapeId.from("test#One"))
        ).transform(model)

        val index = HttpBindingIndex(transformed)
        index.getRequestBindings(ShapeId.from("test#One")).map { (key, value) ->
            key to value.location
        }.toMap() shouldBe mapOf(
            "one" to HttpBinding.Location.HEADER,
            "two" to HttpBinding.Location.QUERY,
            "three" to HttpBinding.Location.QUERY,
            "four" to HttpBinding.Location.QUERY,
        )

        index.getRequestBindings(ShapeId.from("test#Two")).map { (key, value) ->
            key to value.location
        }.toMap() shouldBe mapOf(
            "one" to HttpBinding.Location.HEADER,
            "two" to HttpBinding.Location.QUERY,
            "three" to HttpBinding.Location.DOCUMENT,
            "four" to HttpBinding.Location.DOCUMENT,
        )
    }
}
