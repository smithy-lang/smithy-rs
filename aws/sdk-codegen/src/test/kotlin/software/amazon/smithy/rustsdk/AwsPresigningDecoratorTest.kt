/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.CodegenConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
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
        val transformed = decorator.transformModel(settings(model), serviceShape(model), model)
        hasPresignableTrait(transformed) shouldBe presignable
    }

    private fun hasPresignableTrait(model: Model): Boolean =
        model.shapes().filter { shape -> shape is OperationShape }
            .findFirst()
            .orNull()!!
            .hasTrait(PresignableTrait.ID)

    private fun serviceShape(model: Model): ServiceShape =
        model.shapes().filter { shape -> shape is ServiceShape }.findFirst().orNull()!! as ServiceShape

    private fun settings(model: Model) = RustSettings(
        service = ShapeId.from("notrelevant#notrelevant"),
        moduleName = "test-module",
        moduleVersion = "notrelevant",
        moduleAuthors = listOf("notrelevant"),
        runtimeConfig = RuntimeConfig(),
        codegenConfig = CodegenConfig(eventStreamAllowList = setOf("test-module")),
        license = null,
        model = model
    )

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
