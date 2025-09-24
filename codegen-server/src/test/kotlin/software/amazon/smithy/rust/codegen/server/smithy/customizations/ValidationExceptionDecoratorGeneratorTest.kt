package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ValidationExceptionDecoratorGeneratorTest {

    @Test
    fun `validationExceptionFieldName returns correct field structure name`() {
        val model =
            """
            namespace example.weather

            use smithy.rust.codegen.server.traits#validationException
            use smithy.rust.codegen.server.traits#validationMessage
            use smithy.rust.codegen.server.traits#validationFieldList
            use smithy.rust.codegen.server.traits#validationFieldName
            use smithy.rust.codegen.server.traits#validationFieldMessage

            @validationException
            @error("client")
            structure MyCustomValidationException {
                @validationMessage
                message: String,

                @validationFieldList
                fieldErrors: ValidationFieldList

                errorCode: String,
            }

            list ValidationFieldList {
                member: ValidationField
            }

            structure ValidationField {
                @validationFieldName
                fieldName: String,

                @validationFieldMessage
                errorMessage: String
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val fieldGenerator = ValidationExceptionDecoratorGenerator(codegenContext)

        fieldGenerator.exceptionFieldName shouldBe "ValidationField"
    }

    @Test
    fun `validationExceptionFieldName returns default when no validation field structure exists`() {
        val model =
            """
            namespace example.weather

            use smithy.rust.codegen.server.traits#validationException
            use smithy.rust.codegen.server.traits#validationMessage

            @validationException
            @error("client")
            structure MyCustomValidationException {
                @validationMessage
                message: String,

                errorCode: String,
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val codegenContext = serverTestCodegenContext(model)
        val fieldGenerator = ValidationExceptionDecoratorGenerator(codegenContext)

        fieldGenerator.exceptionFieldName shouldBe "ValidationExceptionField"
    }
}
