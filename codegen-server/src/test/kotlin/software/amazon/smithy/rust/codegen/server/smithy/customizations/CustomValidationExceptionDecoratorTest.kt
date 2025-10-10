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
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.traits.ValidationExceptionTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldListTrait
import software.amazon.smithy.rust.codegen.traits.ValidationFieldNameTrait
import software.amazon.smithy.rust.codegen.traits.ValidationMessageTrait

internal class CustomValidationExceptionDecoratorTest {
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
        return decorator.customValidationException(codegenContext)!!
    }

    @Test
    fun `customValidationException returns correct shape`() {
        val result = mockValidationException(modelWithCustomValidation)

        result shouldNotBe null
        result.id shouldBe ShapeId.from("com.example#MyValidationException")
        result.hasTrait(ValidationExceptionTrait.ID) shouldBe true
    }

    @Test
    fun `customValidationException returns null when no validation exception exists`() {
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

        val result = decorator.customValidationException(codegenContext)

        result shouldBe null
    }

    @Test
    fun `customValidationMessage returns correct member shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.customValidationMessage()

        result shouldNotBe null
        result.memberName shouldBe "customMessage"
        result.hasTrait(ValidationMessageTrait.ID) shouldBe true
    }

    @Test
    fun `customValidationFieldList returns correct member shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.customValidationFieldList()

        result shouldNotBe null
        result!!.memberName shouldBe "customFieldList"
        result.hasTrait(ValidationFieldListTrait.ID) shouldBe true
    }

    @Test
    fun `customValidationFieldList returns null when no field list exists`() {
        val model = modelWithoutFieldList
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.customValidationFieldList()

        result shouldBe null
    }

    @Test
    fun `customValidationExceptionField returns correct structure shape`() {
        val model = modelWithCustomValidation
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.customValidationField()

        result shouldNotBe null
        result!!.id shouldBe ShapeId.from("com.example#ValidationExceptionField")
        result.members().any { it.hasTrait(ValidationFieldNameTrait.ID) } shouldBe true
    }

    @Test
    fun `customValidationExceptionField returns null when no field list exists`() {
        val model = modelWithoutFieldList
        val codegenContext = serverTestCodegenContext(model)
        val generator =
            UserProvidedValidationExceptionConversionGenerator(codegenContext, mockValidationException(model))

        val result = generator.customValidationField()

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
}
