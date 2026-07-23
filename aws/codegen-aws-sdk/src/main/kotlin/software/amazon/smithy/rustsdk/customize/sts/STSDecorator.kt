/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk.customize.sts

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import java.util.logging.Logger

class STSDecorator : ClientCodegenDecorator {
    override val name: String = "STS"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)

    private fun isIdpCommunicationError(shape: Shape): Boolean =
        shape is StructureShape && shape.hasTrait<ErrorTrait>() &&
            shape.id.namespace == "com.amazonaws.sts" && shape.id.name == "IDPCommunicationErrorException"

    private fun isAwsCredentials(shape: Shape): Boolean = shape.id == ShapeId.from("com.amazonaws.sts#Credentials")

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(isIdpCommunicationError(shape)) {
                logger.info("Adding @retryable trait to $shape and setting its error type to 'server'")
                (shape as StructureShape).toBuilder()
                    .removeTrait(ErrorTrait.ID)
                    .addTrait(ErrorTrait("server"))
                    .addTrait(RetryableTrait.builder().build()).build()
            }.letIf(isAwsCredentials(shape)) {
                (shape as StructureShape).toBuilder().addTrait(SensitiveTrait()).build()
            }
        }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations +
            object : OperationCustomization() {
                private val runtimeConfig = codegenContext.runtimeConfig
                private val symbolProvider = codegenContext.symbolProvider

                override fun section(section: OperationSection): Writable =
                    writable {
                        when (section) {
                            // STS clients must treat `IDPCommunicationError` as a transient error
                            // for ALL operations, not only those that model the
                            // `IDPCommunicationErrorException` shape (which the @retryable trait
                            // above already covers). Classify it by error code so every STS
                            // operation retries it.
                            is OperationSection.RetryClassifiers -> {
                                section.registerRetryClassifier(this) {
                                    rustTemplate(
                                        """
                                        #{AwsErrorCodeClassifier}::<#{OperationError}>::builder().transient_errors({
                                            let mut transient_errors: Vec<&'static str> = #{TRANSIENT_ERRORS}.into();
                                            transient_errors.push("IDPCommunicationError");
                                            #{Cow}::Owned(transient_errors)
                                            }).build()""",
                                        "AwsErrorCodeClassifier" to
                                            AwsRuntimeType.awsRuntime(runtimeConfig)
                                                .resolve("retries::classifiers::AwsErrorCodeClassifier"),
                                        "Cow" to RuntimeType.Cow,
                                        "OperationError" to symbolProvider.symbolForOperationError(operation),
                                        "TRANSIENT_ERRORS" to
                                            AwsRuntimeType.awsRuntime(runtimeConfig)
                                                .resolve("retries::classifiers::TRANSIENT_ERRORS"),
                                    )
                                }
                            }

                            else -> {}
                        }
                    }
            }
}
