/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DeprecatedTrait
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.targetOrSelf
import kotlin.jvm.optionals.getOrNull

internal class S3ExpiresDecoratorTest {
    private val baseModel =
        """
        namespace smithy.example
        use aws.protocols#restXml
        use aws.auth#sigv4
        use aws.api#service
        @restXml
        @sigv4(name: "s3")
        @service(
            sdkId: "S3"
            arnNamespace: "s3"
        )
        service S3 {
            version: "1.0.0",
            operations: [GetFoo, NewGetFoo]
        }
        operation GetFoo {
            input: GetFooInput
            output: GetFooOutput
        }

        operation NewGetFoo {
            input: GetFooInput
            output: NewGetFooOutput
        }

        structure GetFooInput {
            payload: String
            expires: String
        }

        @output
        structure GetFooOutput {
            expires: Timestamp
        }

        @output
        structure NewGetFooOutput {
            expires: String
        }
        """.asSmithyModel()

    private val serviceShape = baseModel.expectShape(ShapeId.from("smithy.example#S3"), ServiceShape::class.java)
    private val settings = testClientRustSettings()
    private val model = S3ExpiresDecorator().transformModel(serviceShape, baseModel, settings)

    @Test
    fun `Model is pre-processed correctly`() {
        val expiresShapes =
            listOf(
                model.expectShape(ShapeId.from("smithy.example#GetFooInput\$expires")),
                model.expectShape(ShapeId.from("smithy.example#GetFooOutput\$expires")),
                model.expectShape(ShapeId.from("smithy.example#NewGetFooOutput\$expires")),
            )

        // Expires should always be Timestamp, even if not modeled as such since its
        // type will change in the future
        assertTrue(expiresShapes.all { it.targetOrSelf(model).isTimestampShape })

        // All Expires output members should be marked with the deprecated trait
        assertTrue(
            expiresShapes
                .filter { it.id.toString().contains("Output") }
                .all { it.hasTrait<DeprecatedTrait>() },
        )

        // No ExpiresString member should be added to the input shape
        assertNull(model.getShape(ShapeId.from("smithy.example#GetFooInput\$expiresString")).getOrNull())

        val expiresStringOutputFields =
            listOf(
                model.expectShape(ShapeId.from("smithy.example#GetFooOutput\$expiresString")),
                model.expectShape(ShapeId.from("smithy.example#NewGetFooOutput\$expiresString")),
            )

        // There should be a synthetic ExpiresString string member added to output shapes
        assertTrue(expiresStringOutputFields.all { it.targetOrSelf(model).isStringShape })

        // The synthetic fields should not be deprecated
        assertTrue(expiresStringOutputFields.none { it.hasTrait<DeprecatedTrait>() })
    }
}
