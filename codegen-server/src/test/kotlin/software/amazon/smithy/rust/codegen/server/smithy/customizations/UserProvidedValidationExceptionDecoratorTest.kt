/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.framework.rust.ValidationExceptionTrait
import software.amazon.smithy.framework.rust.ValidationFieldListTrait
import software.amazon.smithy.framework.rust.ValidationFieldNameTrait
import software.amazon.smithy.framework.rust.ValidationMessageTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

internal class UserProvidedValidationExceptionDecoratorTest {
    private val modelWithCustomValidation =
        """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationMessage
        use smithy.framework.rust#validationFieldList
        use smithy.framework.rust#validationFieldName

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
        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationMessage

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
        return decorator.firstStructureShapeWithValidationExceptionTrait(codegenContext.model)!!
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

        val result = decorator.firstStructureShapeWithValidationExceptionTrait(codegenContext.model)

        result shouldBe null
    }

    @Test
    fun `validationMessageMember returns correct member shape`() {
        val model = modelWithCustomValidation
        val validationExceptionStructure = mockValidationException(model)

        val result = UserProvidedValidationExceptionDecorator().validationMessageMember(validationExceptionStructure)

        result shouldNotBe null
        result.memberName shouldBe "customMessage"
        result.hasTrait(ValidationMessageTrait.ID) shouldBe true
    }

    @Test
    fun `validationFieldListMember returns correct member shape`() {
        val model = modelWithCustomValidation
        val validationExceptionStructure = mockValidationException(model)
        val result =
            UserProvidedValidationExceptionDecorator().maybeValidationFieldList(
                model,
                validationExceptionStructure,
            )!!.validationFieldListMember

        result shouldNotBe null
        result.memberName shouldBe "customFieldList"
        result.hasTrait(ValidationFieldListTrait.ID) shouldBe true
    }

    @Test
    fun `maybeValidationFieldList returns null when no field list exists`() {
        val model = modelWithoutFieldList
        val validationExceptionStructure = mockValidationException(model)

        val result =
            UserProvidedValidationExceptionDecorator().maybeValidationFieldList(
                model,
                validationExceptionStructure,
            )

        result shouldBe null
    }

    @Test
    fun `validationFieldStructure returns correct structure shape`() {
        val model = modelWithCustomValidation
        val validationExceptionStructure = mockValidationException(model)

        val result =
            UserProvidedValidationExceptionDecorator().maybeValidationFieldList(
                model,
                validationExceptionStructure,
            )!!.validationFieldStructure

        result shouldNotBe null
        result.id shouldBe ShapeId.from("com.example#ValidationExceptionField")
        result.members().any { it.hasTrait(ValidationFieldNameTrait.ID) } shouldBe true
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
        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationFieldList
        use smithy.framework.rust#validationFieldMessage
        use smithy.framework.rust#validationFieldName
        use smithy.framework.rust#validationMessage

        @restJson1
        service CustomValidationExample {
            version: "1.0.0"
            operations: [
                TestOperation
                StreamingOperation
                PublishMessages
            ]
            errors: [
                MyCustomValidationException
            ]
        }

        @http(method: "POST", uri: "/test")
        operation TestOperation {
            input: TestInput
        }

        @http(method: "GET", uri: "/streaming-operation")
        @readonly
        operation StreamingOperation {
            input := {}
            output := {
                @httpPayload
                output: StreamingBlob = ""
            }
        }

        @streaming
        blob StreamingBlob

        @http(method: "POST", uri: "/publish")
        operation PublishMessages {
            input: PublishMessagesInput
        }

        @input
        structure PublishMessagesInput {
            @httpPayload
            messages: PublishEvents
        }

        @streaming
        union PublishEvents {
            message: Message
            leave: LeaveEvent
        }

        structure Message {
            message: String
        }

        structure LeaveEvent {}

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
        serverIntegrationTest(completeTestModel, testCoverage = HttpTestType.AsConfigured)
    }

    private val completeTestModelWithOptionals =
        """
        namespace com.aws.example

        use aws.protocols#restJson1
        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationFieldList
        use smithy.framework.rust#validationFieldMessage
        use smithy.framework.rust#validationFieldName
        use smithy.framework.rust#validationMessage

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
        serverIntegrationTest(completeTestModelWithOptionals, testCoverage = HttpTestType.AsConfigured)
    }

    private val completeTestModelWithImplicitNamesWithoutFieldMessage =
        """
        namespace com.aws.example

        use aws.protocols#restJson1
        use smithy.framework.rust#validationException
        use smithy.framework.rust#validationFieldList

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
            message: String

            @validationFieldList
            customFieldList: CustomValidationFieldList
        }

        structure CustomValidationField {
            name: String,
        }

        list CustomValidationFieldList {
            member: CustomValidationField
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `code compiles with implicit message and field name and without field message`() {
        serverIntegrationTest(completeTestModelWithImplicitNamesWithoutFieldMessage)
    }
}
