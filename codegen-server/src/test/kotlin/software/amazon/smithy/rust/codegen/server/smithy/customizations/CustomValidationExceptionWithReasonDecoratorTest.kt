/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import java.nio.file.Path
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

        for (schemaSerde in listOf(false, true)) {
            val servers =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            Node.objectNodeBuilder().withMember(
                                "codegen",
                                Node.objectNodeBuilder()
                                    .withMember("schemaSerde", schemaSerde)
                                    .withMember("http-1x", schemaSerde)
                                    .withMember("experimentalCustomValidationExceptionWithReasonPleaseDoNotUse", "com.amazonaws.constraints#ValidationException")
                                    .build(),
                            ).build(),
                    ),
                    testCoverage = HttpTestType.Default,
                )
            servers.forEach {
                it.path.resolve("src/protocol_serde").toFile().exists() shouldBe !schemaSerde
                assertValidationExceptionConversion(it.path, schemaSerde)
            }
        }
    }
}

internal fun assertValidationExceptionConversion(
    crate: Path,
    schemaHttpSerde: Boolean,
) {
    val conversion = Regex("""impl\s+::std::convert::From<ConstraintViolation>\s+for\s+crate::error::\w+\s*\{""")
    conversion.containsMatchIn(crate.resolve("src/input.rs").toFile().readText()) shouldBe schemaHttpSerde
}

internal class ValidationExceptionConversionTest {
    @Test
    fun `validation conversion is confined to schema HTTP serde`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test
            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service Example {
                version: "1"
                operations: [Check]
            }

            @http(method: "POST", uri: "/check")
            operation Check {
                input: CheckInput
                errors: [ValidationException]
            }

            structure CheckInput {
                @required
                @length(min: 1)
                name: String
            }
            """.asSmithyModel()

        for ((http1x, schemaSerde) in listOf(false to null, false to false, true to null, true to false, true to true)) {
            var codegen = Node.objectNode().withMember("http-1x", http1x)
            if (schemaSerde != null) {
                codegen = codegen.withMember("schemaSerde", schemaSerde)
            }
            val schemaHttpSerde = http1x && schemaSerde == true
            val servers =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = Node.objectNode().withMember("codegen", codegen),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, rustCrate ->
                    val operation = context.model.lookup<OperationShape>("test#Check")
                    val input = context.symbolProvider.toSymbol(operation.inputShape(context.model))
                    val protocol = ServerRestJsonProtocol(context)
                    rustCrate.testModule {
                        unitTest("missing_required_member_retains_validation_details") {
                            rustTemplate(
                                """
                                let violation = #{Input}::builder().build().expect_err("missing required member");
                                """,
                                "Input" to input,
                            )
                            if (schemaHttpSerde) {
                                rustTemplate(
                                    """
                                    let exception: #{ValidationException} = violation.into();
                                    assert!(exception.message.contains("1 validation error detected."));
                                    assert_eq!(exception.field_list.unwrap()[0].path, "/name");
                                    """,
                                    "ValidationException" to context.symbolProvider.toSymbol(context.model.expectShape(SmithyValidationExceptionConversionGenerator.SHAPE_ID)),
                                )
                            } else {
                                rustTemplate(
                                    """
                                    let rejection: #{RequestRejection} = violation.into();
                                    if let #{RequestRejection}::ConstraintViolation(body) = rejection {
                                        assert!(body.contains("1 validation error detected."));
                                        assert!(body.contains("fieldList"));
                                        assert!(body.contains("\"/name\""));
                                    } else {
                                        panic!("expected a serialized validation exception");
                                    }
                                    """,
                                    "RequestRejection" to protocol.requestRejection(context.runtimeConfig),
                                )
                            }
                        }
                    }
                }
            servers.forEach { assertValidationExceptionConversion(it.path, schemaHttpSerde) }
        }
    }
}
