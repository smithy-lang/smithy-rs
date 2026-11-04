/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Implements the older validation-exception decorator interface and verifies that generated
 * legacy servers still convert constraint violations into the expected HTTP validation error
 * for both HTTP library versions. Enabling schema serde with an incomplete decorator must fail
 * codegen with a useful diagnostic naming the missing schema conversion or exception symbol.
 */
internal class LegacyValidationExceptionConversionTest {
    private val model =
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

    private fun decorator(withSymbol: Boolean = false): ServerCodegenDecorator =
        object : ServerCodegenDecorator {
            override val name = "LegacyValidationExceptionDecorator"
            override val order: Byte = -1

            override fun validationExceptionConversion(
                codegenContext: ServerCodegenContext,
            ): ValidationExceptionConversionGenerator =
                if (withSymbol) {
                    object : LegacyGenerator(codegenContext) {
                        override fun validationExceptionSymbol(): Symbol = delegate.validationExceptionSymbol()
                    }
                } else {
                    LegacyGenerator(codegenContext)
                }
        }

    private fun params(schemaSerde: Boolean) =
        IntegrationTestParams(
            additionalSettings =
                ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
            cargoCommand = "cargo test --quiet --lib --tests",
        )

    @Test
    fun `old interface implementation retains legacy validation conversion for both HTTP versions`() {
        var legacyConversions = 0
        val servers =
            serverIntegrationTest(model, params(false), additionalDecorators = listOf(decorator())) { context, rustCrate ->
                context.usesSchemaHttpSerde shouldBe false
                legacyConversions++
                rustCrate.testModule {
                    unitTest("legacy_validation_exception_retains_details") {
                        rustTemplate(
                            """
                            let violation = crate::input::CheckInput::builder().build().expect_err("missing required member");
                            let rejection: #{RequestRejection} = violation.into();
                            if let #{RequestRejection}::ConstraintViolation(body) = rejection {
                                assert!(body.contains("1 validation error detected."));
                                assert!(body.contains("fieldList"));
                                assert!(body.contains("\"/name\""));
                            } else {
                                panic!("expected a serialized validation exception");
                            }
                            """,
                            "RequestRejection" to ServerRestJsonProtocol(context).requestRejection(context.runtimeConfig),
                        )
                    }
                }
            }
        legacyConversions shouldBe 2
        servers.forEach { assertValidationExceptionConversion(it.path, false) }
    }

    @Test
    fun `schema generation diagnoses a missing validation exception symbol`() {
        val exception =
            assertThrows<CodegenException> {
                serverIntegrationTest(
                    model,
                    params(true),
                    additionalDecorators = listOf(decorator()),
                    testCoverage = HttpTestType.Default,
                )
            }
        exception.message shouldContain LegacyGenerator::class.java.name
        exception.message shouldContain "smithy.framework#ValidationException"
        exception.message shouldContain "Implement validationExceptionSymbol()"
        exception.message shouldContain "set codegen.schemaSerde to false"
    }

    @Test
    fun `schema generation diagnoses a missing validation exception conversion`() {
        val exception =
            assertThrows<CodegenException> {
                serverIntegrationTest(
                    model,
                    params(true),
                    additionalDecorators = listOf(decorator(withSymbol = true)),
                    testCoverage = HttpTestType.Default,
                )
            }
        exception.message shouldContain "smithy.framework#ValidationException"
        exception.message shouldContain "Implement renderImplFromConstraintViolationForValidationException()"
        exception.message shouldContain "set codegen.schemaSerde to false"
    }

    // Implement only the original interface members. Explicit forwarding ensures that neither
    // schema method is inherited from the built-in generator through Kotlin interface delegation.
    private open class LegacyGenerator(codegenContext: ServerCodegenContext) : ValidationExceptionConversionGenerator {
        protected val delegate = SmithyValidationExceptionConversionGenerator(codegenContext)
        override val shapeId = delegate.shapeId

        override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
            delegate.renderImplFromConstraintViolationForRequestRejection(protocol)

        override fun stringShapeConstraintViolationImplBlock(
            stringConstraintsInfo: Collection<StringTraitInfo>,
        ): Writable = delegate.stringShapeConstraintViolationImplBlock(stringConstraintsInfo)

        override fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait): Writable =
            delegate.enumShapeConstraintViolationImplBlock(enumTrait)

        override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable =
            delegate.numberShapeConstraintViolationImplBlock(rangeInfo)

        override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable =
            delegate.blobShapeConstraintViolationImplBlock(blobConstraintsInfo)

        override fun mapShapeConstraintViolationImplBlock(
            shape: MapShape,
            keyShape: StringShape,
            valueShape: Shape,
            symbolProvider: RustSymbolProvider,
            model: Model,
        ): Writable = delegate.mapShapeConstraintViolationImplBlock(shape, keyShape, valueShape, symbolProvider, model)

        override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable =
            delegate.builderConstraintViolationFn(constraintViolations)

        override fun collectionShapeConstraintViolationImplBlock(
            collectionConstraintsInfo: Collection<CollectionTraitInfo>,
            isMemberConstrained: Boolean,
        ): Writable =
            delegate.collectionShapeConstraintViolationImplBlock(collectionConstraintsInfo, isMemberConstrained)

        override fun unionShapeConstraintViolationImplBlock(
            unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
        ): Writable = delegate.unionShapeConstraintViolationImplBlock(unionConstraintTraitInfo)
    }
}
