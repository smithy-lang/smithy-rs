/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.collections.shouldHaveSize
import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

internal class CustomValidationExceptionDecoratorTest {
    @Test
    fun `decorator returns generator when custom validation exception exists`() {
        val model =
            """
            namespace com.example

            use smithy.rust.codegen.server.traits#validationException
            use smithy.rust.codegen.server.traits#validationMessage

            @validationException
            @error("client")
            structure MyValidationException {
                @validationMessage
                message: String
                
                fieldList: ValidationExceptionFieldList
                
                @default
                errorCode: String
                
                @default
                timestamp: Timestamp
            }

            structure ValidationExceptionField {
                @default
                message: String
                @default
                path: String
            }

            list ValidationExceptionFieldList {
                member: ValidationExceptionField
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val decorator = CustomValidationExceptionDecorator()

        val generator = decorator.validationExceptionConversion(codegenContext)

        generator shouldNotBe null
        codegenContext.customExceptionName shouldBe "MyValidationException"

        // Test helper methods
        val conversionGenerator = generator as CustomValidationExceptionConversionGenerator
        val customShape = conversionGenerator.getCustomExceptionShape()
        customShape shouldNotBe null
        customShape!!.id.name shouldBe "MyValidationException"

        val additionalMembers = conversionGenerator.getAdditionalMembers()
        additionalMembers shouldHaveSize 2
        additionalMembers.map { it.memberName }.toSet() shouldBe setOf("errorCode", "timestamp")
    }

    @Test
    fun `decorator returns null when no custom validation exception exists`() {
        val model =
            """
            namespace com.example

            structure RegularException {
                message: String
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val decorator = CustomValidationExceptionDecorator()

        val generator = decorator.validationExceptionConversion(codegenContext)

        generator shouldBe null
        codegenContext.customExceptionName shouldBe null
    }

//    @Test
//    fun `generates custom validation exception builder code`() {
//        val model =
//            """
//            namespace com.example
//
//            use smithy.rust.codegen.server.traits#validationException
//            use smithy.rust.codegen.server.traits#validationMessage
//
//            @validationException
//            @error("client")
//            structure MyValidationException {
//                @validationMessage
//                message: String
//
//                @default
//                errorCode: String
//            }
//            """.asSmithyModel(smithyVersion = "2.0")
//
//        val codegenContext = serverTestCodegenContext(model)
//        val project = TestWorkspace.testProject()
//
//        project.withModule(ServerRustModule.Error) {
//            val decorator = CustomValidationExceptionDecorator()
//            val generator = decorator.validationExceptionConversion(codegenContext) as CustomValidationExceptionConversionGenerator
//
//            // Generate our custom builder code
//            generator.generateCustomValidationExceptionBuilder()(this)
//        }
//
//        project.compileAndTest()
//    }
}
