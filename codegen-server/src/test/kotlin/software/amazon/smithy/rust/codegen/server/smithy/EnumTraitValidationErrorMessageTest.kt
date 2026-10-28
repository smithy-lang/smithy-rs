/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.server.smithy.customizations.swapOutSmithyValidationExceptionForCustomOne
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class EnumTraitValidationErrorMessageTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test

        use aws.protocols#restJson1
        use smithy.framework#ValidationException

        @restJson1
        service TestService {
            operations: [TestOperation]
        }

        @http(uri: "/test", method: "POST")
        operation TestOperation {
            input := {
                string: EnumString
                stringWithEnumTrait: EnumTraitString
            }
            errors: [ValidationException]
        }

        enum EnumString {
            ABC = "abc"

            DEF = "def"

            @internal
            GHI = "ghi"

            @tags(["internal"])
            JKL = "jkl"
        }

        @suppress(["ModelDeprecation"])
        @enum([
            {
                value: "abc"
                name: "ABC"
                tags: ["external"]
            }
            {
                value: "def"
                name: "DEF"
            }
            {
                value: "ghi"
                name: "GHI"
                tags: ["internal"]
            }
        ])
        string EnumTraitString
        """.asSmithyModel()

    @ParameterizedTest
    @CsvSource(delimiter = '|', value = ["test#EnumString|abc, def, jkl", "test#EnumTraitString|abc, def"])
    fun `internal enum values are excluded from validation messages`(
        shapeId: String,
        expectedValues: String,
    ) {
        val shape = model.expectShape(ShapeId.from(shapeId))
        val enumTrait = shape.expectTrait<EnumTrait>().withoutInternalValues(shape)

        enumTrait.validationErrorMessage() shouldBe
            "Value at '{}' failed to satisfy constraint: Member must satisfy enum value set: [$expectedValues]"
        enumTrait.shapeConstraintViolationDisplayMessage(shape) shouldBe
            "Value provided for '$shapeId' failed to satisfy constraint: Member must satisfy enum value set: [$expectedValues]"
    }

    private fun generatedCodeTest(
        model: Model,
        params: IntegrationTestParams,
    ) {
        serverIntegrationTest(model, params, testCoverage = HttpTestType.Default) { _, rustCrate ->
            rustCrate.testModule {
                for ((shape, path, expectedValues) in listOf(
                    Triple("EnumString", "string", "abc, def, jkl"),
                    Triple("EnumTraitString", "string_with_enum_trait", "abc, def"),
                )) {
                    unitTest(
                        "${path}_validation_message_excludes_internal_values",
                        """
                        let violation = crate::model::$shape::try_from("xyz").unwrap_err();
                        assert_eq!(
                            violation.to_string(),
                            "Value provided for 'test#$shape' failed to satisfy constraint: Member must satisfy enum value set: [$expectedValues]",
                        );
                        assert_eq!(
                            violation.as_validation_exception_field("/$path".to_owned()).message,
                            "Value at '/$path' failed to satisfy constraint: Member must satisfy enum value set: [$expectedValues]",
                        );
                        assert_eq!(crate::model::$shape::try_from("ghi").unwrap(), crate::model::$shape::Ghi);
                        """,
                    )
                }
            }
        }
    }

    @Test
    fun `generated validation exception messages exclude internal enum values`() {
        generatedCodeTest(model, IntegrationTestParams())
    }

    @Test
    fun `generated custom validation exception with reason messages exclude internal enum values`() {
        generatedCodeTest(
            swapOutSmithyValidationExceptionForCustomOne(model),
            IntegrationTestParams(
                additionalSettings =
                    Node.objectNodeBuilder().withMember(
                        "codegen",
                        Node.objectNodeBuilder()
                            .withMember(
                                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse",
                                "com.amazonaws.constraints#ValidationException",
                            )
                            .build(),
                    ).build(),
            ),
        )
    }
}
