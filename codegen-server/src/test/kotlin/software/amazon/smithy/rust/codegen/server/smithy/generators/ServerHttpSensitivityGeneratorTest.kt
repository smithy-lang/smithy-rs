/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.HttpResponseCodeTrait
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.inputShape

class ServerHttpSensitivityGeneratorTest {
    private val outerSensitiveModel = """
        namespace test

        operation Secret {
            input: Input,
        }

        @sensitive
        structure Input {
            @required
            @httpResponseCode
            code: Integer,
        }
    """.asSmithyModel()

    @Test
    fun `find outer sensitive`() {
        val operation = outerSensitiveModel.getOperationShapes().toList()[0]
        val inputShape = operation.inputShape(outerSensitiveModel)
        val generator = ServerHttpSensitivityGenerator(outerSensitiveModel, operation, TestRuntimeConfig)
        val members: List<String> = generator.findSensitiveBound<HttpResponseCodeTrait>(inputShape).map(MemberShape::getMemberName)
        assertEquals(members, listOf("code"))
    }

    private val innerSensitiveModel = """
        namespace test

        operation Secret {
            input: Input,
        }

        structure Input {
            @required
            @sensitive
            @httpHeader("header-a")
            a: String,

            @required
            @httpHeader("header-b")
            b: String
        }
    """.asSmithyModel()

    @Test
    fun `find inner sensitive`() {
        val operation = innerSensitiveModel.getOperationShapes().toList()[0]
        val inputShape = operation.inputShape(innerSensitiveModel)
        val generator = ServerHttpSensitivityGenerator(innerSensitiveModel, operation, TestRuntimeConfig)
        val members: List<String> = generator.findSensitiveBound<HttpHeaderTrait>(inputShape).map(MemberShape::getMemberName)
        assertEquals(members, listOf("a"))
    }

    private val nestedSensitiveModel = """
        namespace test

        operation Secret {
            input: Input,
        }

        @sensitive
        structure Input {
            @required
            @httpHeader("header-a")
            a: String,

            nested: Nested
        }

        structure Nested {
            @required
            @httpHeader("header-b")
            b: String
        }
    """.asSmithyModel()

    @Test
    fun `find nested sensitive`() {
        val operation = nestedSensitiveModel.getOperationShapes().toList()[0]
        val inputShape = operation.inputShape(nestedSensitiveModel)
        val generator = ServerHttpSensitivityGenerator(nestedSensitiveModel, operation, TestRuntimeConfig)
        val members: List<String> = generator.findSensitiveBound<HttpHeaderTrait>(inputShape).map(MemberShape::getMemberName)
        assertEquals(members, listOf("b", "a"))
    }
}
