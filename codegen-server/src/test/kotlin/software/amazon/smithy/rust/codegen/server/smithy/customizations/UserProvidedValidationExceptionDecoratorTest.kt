/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldListTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldNameTrait
import software.amazon.smithy.rust.codegen.traits.ValidationMessageTrait

internal class UserProvidedValidationExceptionDecoratorTest {
    private val modelWithCustomValidation =
        """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationMessage
        use smithy.rust.codegen.traits#validationFieldList
        use smithy.rust.codegen.traits#validationFieldName

        @restJson1
        service TestService {
            version: "1.0.0"
        }

        @validationException
        @error("client")
        structure MyValidationException {
            @validationMessage
            customMessage: String

            @validationFieldList
            customFieldList: ValidationExceptionFieldList
        }

        structure ValidationExceptionField {
            @validationFieldName
            path: String
            message: String
        }

        list ValidationExceptionFieldList {
            member: ValidationExceptionField
        }
        """.asSmithyModel(smithyVersion = "2.0")

    private val modelWithoutFieldList =
        """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationMessage

        @restJson1
        service TestService {
            version: "1.0.0"
        }

        @validationException
        @error("client")
        structure MyValidationException {
            @validationMessage
            message: String
        }
        """.asSmithyModel(smithyVersion = "2.0")

    private fun mockValidationException(model: Model): StructureShape {
        val codegenContext = serverTestCodegenContext(model)
        val decorator = UserProvidedValidationExceptionDecorator()
        return decorator.firstStructureShapeWithValidationExceptionTrait(codegenContext)!!
    }

    @Test
    fun `firstStructureShapeWithValidationExceptionTrait returns correct shape`() {
        val result = mockValidationException(modelWithCustomValidation)

        result shouldNotBe null
        result.id shouldBe ShapeId.from("com.example#MyValidationException")
        result.hasTrait(ValidationExceptionTrait.ID) shouldBe true
    }

    @Test
    fun `firstStructureShapeWithValidationExceptionTrait returns null when no validation exception exists`() {
        val model =
            """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "1.0.0"
            }

            structure RegularException { message: String }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val decorator = UserProvidedValidationExceptionDecorator()

        val result = decorator.firstStructureShapeWithValidationExceptionTrait(codegenContext)

        result shouldBe null
    }

    @Test
    fun `userProvidedValidationMessage returns correct member shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.userProvidedValidationMessageMember()

        result shouldNotBe null
        result.memberName shouldBe "customMessage"
        result.hasTrait(ValidationMessageTrait.ID) shouldBe true
    }

    @Test
    fun `userProvidedValidationFieldList returns correct member shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.userProvidedValidationFieldListMember()

        result shouldNotBe null
        result!!.memberName shouldBe "customFieldList"
        result.hasTrait(ValidationFieldListTrait.ID) shouldBe true
    }

    @Test
    fun `userProvidedValidationFieldList returns null when no field list exists`() {
        val model = modelWithoutFieldList
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.userProvidedValidationFieldListMember()

        result shouldBe null
    }

    @Test
    fun `userProvidedValidationField returns correct structure shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.userProvidedValidationFieldStructure()

        result shouldNotBe null
        result!!.id shouldBe ShapeId.from("com.example#ValidationExceptionField")
        result.members().any { it.hasTrait(ValidationFieldNameTrait.ID) } shouldBe true
    }

    @Test
    fun `userProvidedValidationField returns null when no field list exists`() {
        val model = modelWithoutFieldList
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.userProvidedValidationFieldStructure()

        result shouldBe null
    }

    @Test
    fun `decorator returns null when no custom validation exception exists`() {
        val model =
            """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "1.0.0"
            }

            structure RegularException { message: String }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val decorator = UserProvidedValidationExceptionDecorator()

        val generator = decorator.validationExceptionConversion(codegenContext)

        generator shouldBe null
    }

    private val completeTestModel =
        """
        namespace com.aws.example

        use aws.protocols#restJson1
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationFieldList
        use smithy.rust.codegen.traits#validationFieldMessage
        use smithy.rust.codegen.traits#validationFieldName
        use smithy.rust.codegen.traits#validationMessage

        @restJson1
        service CustomValidationExample {
            version: "1.0.0"
            operations: [
                TestOperation
            ]
            errors: [
                MyCustomValidationException
            ]
        }

        @http(method: "POST", uri: "/test")
        operation TestOperation {
            input: TestInput
        }

        structure TestInput {
            @required
            @length(min: 1, max: 10)
            name: String

            @range(min: 1, max: 100)
            age: Integer
        }

        @error("client")
        @httpError(400)
        @validationException
        structure MyCustomValidationException {
            @required
            @validationMessage
            customMessage: String

            @required
            @default("testReason1")
            reason: ValidationExceptionReason

            @validationFieldList
            customFieldList: CustomValidationFieldList
        }

        enum ValidationExceptionReason {
            TEST_REASON_0 = "testReason0"
            TEST_REASON_1 = "testReason1"
        }

        structure CustomValidationField {
            @required
            @validationFieldName
            customFieldName: String

            @required
            @validationFieldMessage
            customFieldMessage: String
        }

        list CustomValidationFieldList {
            member: CustomValidationField
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `code compiles with custom validation exception`() {
        serverIntegrationTest(completeTestModel)
    }

    private val completeTestModelWithOptionals =
        """
        namespace com.aws.example

        use aws.protocols#restJson1
        use smithy.rust.codegen.traits#validationException
        use smithy.rust.codegen.traits#validationFieldList
        use smithy.rust.codegen.traits#validationFieldMessage
        use smithy.rust.codegen.traits#validationFieldName
        use smithy.rust.codegen.traits#validationMessage

        @restJson1
        service CustomValidationExample {
            version: "1.0.0"
            operations: [
                TestOperation
            ]
            errors: [
                MyCustomValidationException
            ]
        }

        @http(method: "POST", uri: "/test")
        operation TestOperation {
            input: TestInput
        }

        structure TestInput {
            @required
            @length(min: 1, max: 10)
            name: String

            @range(min: 1, max: 100)
            age: Integer
        }

        @error("client")
        @httpError(400)
        @validationException
        structure MyCustomValidationException {
            @validationMessage
            customMessage: String

            @default("testReason1")
            reason: ValidationExceptionReason

            @validationFieldList
            customFieldList: CustomValidationFieldList
        }

        enum ValidationExceptionReason {
            TEST_REASON_0 = "testReason0"
            TEST_REASON_1 = "testReason1"
        }

        structure CustomValidationField {
            @validationFieldName
            customFieldName: String

            @validationFieldMessage
            customFieldMessage: String
        }

        list CustomValidationFieldList {
            member: CustomValidationField
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `code compiles with custom validation exception using optionals`() {
        serverIntegrationTest(completeTestModelWithOptionals)
    }
}
