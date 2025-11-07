/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import kotlin.streams.toList

fun swapOutSmithyValidationExceptionForCustomOne(model: Model): Model {
    val customValidationExceptionModel =
        """
        namespace com.amazonaws.constraints

        enum ValidationExceptionFieldReason {
            LENGTH_NOT_VALID = "LengthNotValid"
            PATTERN_NOT_VALID = "PatternNotValid"
            SYNTAX_NOT_VALID = "SyntaxNotValid"
            VALUE_NOT_VALID = "ValueNotValid"
            OTHER = "Other"
        }

        /// Stores information about a field passed inside a request that resulted in an exception.
        structure ValidationExceptionField {
            /// The field name.
            @required
            Name: String

            @required
            Reason: ValidationExceptionFieldReason

            /// Message describing why the field failed validation.
            @required
            Message: String
        }

        /// A list of fields.
        list ValidationExceptionFieldList {
            member: ValidationExceptionField
        }

        enum ValidationExceptionReason {
            FIELD_VALIDATION_FAILED = "FieldValidationFailed"
            UNKNOWN_OPERATION = "UnknownOperation"
            CANNOT_PARSE = "CannotParse"
            OTHER = "Other"
        }

        /// The input fails to satisfy the constraints specified by an AWS service.
        @error("client")
        @httpError(400)
        structure ValidationException {
            /// Description of the error.
            @required
            Message: String

            /// Reason the request failed validation.
            @required
            Reason: ValidationExceptionReason

            /// The field that caused the error, if applicable. If more than one field
            /// caused the error, pick one and elaborate in the message.
            Fields: ValidationExceptionFieldList
        }
        """.asSmithyModel(smithyVersion = "2.0")

    // Remove Smithy's `ValidationException`.
    var model =
        ModelTransformer.create().removeShapes(
            model,
            listOf(model.expectShape(SmithyValidationExceptionConversionGenerator.SHAPE_ID)),
        )
    // Add our custom one.
    model = ModelTransformer.create().replaceShapes(model, customValidationExceptionModel.shapes().toList<Shape>())
    // Make all operations use our custom one.
    val newOperationShapes =
        model.operationShapes.map { operationShape ->
            operationShape.toBuilder().addError(ShapeId.from("com.amazonaws.constraints#ValidationException")).build()
        }
    return ModelTransformer.create().replaceShapes(model, newOperationShapes)
}

internal class CustomValidationExceptionWithReasonDecoratorTest {
    @Test
    fun `constraints model with the CustomValidationExceptionWithReasonDecorator applied compiles`() {
        var model = File("../codegen-core/common-test-models/constraints.smithy").readText().asSmithyModel()
        model = swapOutSmithyValidationExceptionForCustomOne(model)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    Node.objectNodeBuilder().withMember(
                        "codegen",
                        Node.objectNodeBuilder()
                            .withMember("experimentalCustomValidationExceptionWithReasonPleaseDoNotUse", "com.amazonaws.constraints#ValidationException")
                            .build(),
                    ).build(),
            ),
            testCoverage = HttpTestType.AS_CONFIGURED,
        )
    }
}
