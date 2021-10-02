/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.s3

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Disabled
import org.junit.jupiter.api.Nested
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

class S3DecoratorTest {
    @Nested
    inner class S3CorrectSizeIntegerTypeTest {
        // TODO: Re-enable test below once https://github.com/awslabs/smithy/pull/900 is merged and
        // the latest Smithy is pulled in.
        @Disabled
        @Test
        fun `it should make Size a Long`() {
            val model = """
                namespace com.amazonaws.s3

                // The actual model doesn't have traits, but make sure we copy them to stay future-proof
                @range(min: 0)
                integer Size

                structure Object {
                    size: Size,
                }
            """.asSmithyModel()

            // Precondition: make sure the test model is correct
            val oldSize = model.expectShape(S3CorrectSizeIntegerType.SIZE_SHAPE_ID)

            val transformed = S3CorrectSizeIntegerType().transform(model)
            val newSize = transformed.expectShape(S3CorrectSizeIntegerType.SIZE_SHAPE_ID)
            newSize.isLongShape shouldBe true
            newSize.allTraits shouldBe oldSize.allTraits
            newSize.sourceLocation shouldBe oldSize.sourceLocation

            transformed.expectShape(ShapeId.from("com.amazonaws.s3#Object")).members().first().isLongShape shouldBe true
        }
    }
}
