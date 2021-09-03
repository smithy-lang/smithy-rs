/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rustsdk.traits.PresignableTrait

private val PRESIGNABLE_OPERATIONS = listOf(
    // TODO(PresignedReqPrototype): Add the other presignable operations
    ShapeId.from("com.amazonaws.s3#GetObject"),
    ShapeId.from("com.amazonaws.s3#PutObject"),
)

// TODO(PresignedReqPrototype): Write unit test
class AwsPresigningDecorator : RustCodegenDecorator {
    override val name: String = "AwsPresigning"
    override val order: Byte = 0

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> = listOf(
        AwsInputPresignedMethod(protocolConfig.runtimeConfig, protocolConfig.symbolProvider, operation)
    )

    /** Adds presignable trait to known presignable operations */
    override fun transformModel(service: ServiceShape, model: Model): Model {
        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && PRESIGNABLE_OPERATIONS.contains(shape.id)) {
                shape.toBuilder().addTrait(PresignableTrait()).build()
            } else {
                shape
            }
        }
    }
}

class AwsInputPresignedMethod(
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val operationShape: OperationShape
) : OperationCustomization() {
    private val codegenScope = arrayOf(
        "Error" to AwsRuntimeType.Presigning.member("config::Error"),
        "PresignedRequest" to AwsRuntimeType.Presigning.member("request::PresignedRequest"),
        "PresigningConfig" to AwsRuntimeType.Presigning.member("config::PresigningConfig"),
        "SharedConfig" to RuntimeType("Config", runtimeConfig.awsRuntimeDependency("aws-types"), "aws_types::config"),
        "SdkError" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("result::SdkError")
    )

    override fun section(section: OperationSection): Writable = writable {
        if (section is OperationSection.InputImpl && section.operationShape.hasTrait(PresignableTrait::class.java)) {
            writeInputPresignedMethod()
        }
    }

    private fun RustWriter.writeInputPresignedMethod() {
        val operationError = operationShape.errorSymbol(symbolProvider)
        rustBlockTemplate(
            """
            // TODO(PresignedReqPrototype): Doc comments
            pub async fn presigned(
                self,
                config: &#{SharedConfig},
                _presigning_config: #{PresigningConfig}
            ) -> Result<#{PresignedRequest}, #{SdkError}<#{OpError}>>
            """,
            *codegenScope,
            "OpError" to operationError
        ) {
            rustTemplate(
                """
                let (_request, _) = self.make_operation(&config.into())
                    .map_err(|err| #{SdkError}::ConstructionFailure(err.into()))?
                    .into_request_response();
                unimplemented!("TODO(PresignedReqPrototype): middleware chain to construct presigned request")
                """,
                *codegenScope
            )
        }
    }
}
