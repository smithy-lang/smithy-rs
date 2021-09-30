/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientSection
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBoundProtocolBodyWriter
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rustsdk.traits.PresignableTrait
import java.util.stream.Collectors

private enum class PayloadSigningType {
    EMPTY,
    UNSIGNED_PAYLOAD,
}

private data class PresignableOperation(
    val payloadSigningType: PayloadSigningType,
    val modelTransform: PresignModelTransform? = null
)

private val PRESIGNABLE_OPERATIONS by lazy {
    mapOf(
        ShapeId.from("com.amazonaws.s3#GetObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.s3#PutObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.polly#SynthesizeSpeech") to PresignableOperation(
            PayloadSigningType.EMPTY,
            AwsPollySynthesizeSpeechPresignTransform()
        ),
    )
}

class AwsPresigningDecorator : RustCodegenDecorator {
    companion object {
        const val ORDER: Byte = 0
    }

    override val name: String = "AwsPresigning"
    override val order: Byte = ORDER

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        val hasPresignedOps = codegenContext.model.shapes().anyMatch { shape ->
            shape is OperationShape && PRESIGNABLE_OPERATIONS.containsKey(shape.id)
        }
        if (hasPresignedOps) {
            rustCrate.mergeFeature(Feature("client", default = true, listOf("tower")))
        }
    }

    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> = baseCustomizations + listOf(AwsInputPresignedMethod(codegenContext, operation))

    /** Adds presignable trait to known presignable operations */
    override fun transformModel(service: ServiceShape, model: Model): Model {
        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && PRESIGNABLE_OPERATIONS.containsKey(shape.id)) {
                shape.toBuilder().addTrait(PresignableTrait()).build()
            } else {
                shape
            }
        }
    }
}

class AwsInputPresignedMethod(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    private val codegenScope = arrayOf(
        "aws_hyper" to runtimeConfig.awsRuntimeDependency("aws-hyper").copy(optional = true).asType(),
        "Error" to AwsRuntimeType.Presigning.member("config::Error"),
        "PresignedRequest" to AwsRuntimeType.Presigning.member("request::PresignedRequest"),
        "PresignedRequestService" to AwsRuntimeType.Presigning.member("service::PresignedRequestService"),
        "PresigningConfig" to AwsRuntimeType.Presigning.member("config::PresigningConfig"),
        "SdkError" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("result::SdkError"),
        "sig_auth" to runtimeConfig.sigAuth().asType(),
        "tower" to CargoDependency.Tower.asType(),
    )

    override fun section(section: OperationSection): Writable = writable {
        if (section is OperationSection.InputImpl && section.operationShape.hasTrait<PresignableTrait>()) {
            writeInputPresignedMethod(section)
        }
    }

    private fun RustWriter.writeInputPresignedMethod(section: OperationSection) {
        val operationError = operationShape.errorSymbol(symbolProvider)
        val presignableOp = PRESIGNABLE_OPERATIONS.getValue(operationShape.id)

        var makeOperationFn = "make_operation"
        if (presignableOp.modelTransform != null) {
            makeOperationFn = "_make_presigned_operation"

            val transformedModel = presignableOp.modelTransform.transform(codegenContext.model)
            val transformedProtocolConfig = codegenContext.copy(model = transformedModel)
            val transformedOperationShape = transformedModel.expectShape(operationShape.id, OperationShape::class.java)

            val protocol = ProtocolLoader.Default.protocolFor(
                transformedModel,
                transformedProtocolConfig.serviceShape
            ).second.protocol(transformedProtocolConfig)

            MakeOperationGenerator(
                transformedProtocolConfig,
                protocol,
                HttpBoundProtocolBodyWriter(transformedProtocolConfig, protocol),
                // Prefixed with underscore to avoid colliding with modeled functions
                functionName = makeOperationFn,
                public = false,
            ).generateMakeOperation(this, transformedOperationShape, section.customizations)
        }

        val payloadSigningType = when (presignableOp.payloadSigningType) {
            PayloadSigningType.EMPTY -> "Empty"
            PayloadSigningType.UNSIGNED_PAYLOAD -> "UnsignedPayload"
        }
        rustBlockTemplate(
            """
            /// Creates a presigned request for this operation. The credentials provider from the `config`
            /// will be used to generate the request's signature, and the `presigning_config` provides additional
            /// presigning-specific config values, such as the amount of time the request should be valid for after
            /// creation.
            ///
            /// Presigned requests can be given to other users or applications to access a resource or perform
            /// an operation without having access to the AWS security credentials.
            ##[cfg(feature = "client")]
            pub async fn presigned(
                self,
                config: &crate::config::Config,
                presigning_config: #{PresigningConfig}
            ) -> Result<#{PresignedRequest}, #{SdkError}<#{OpError}>>
            """,
            *codegenScope,
            "OpError" to operationError
        ) {
            rustTemplate(
                """
                let (mut request, _) = self.$makeOperationFn(config)
                    .map_err(|err| #{SdkError}::ConstructionFailure(err.into()))?
                    .into_request_response();

                // Change signature type to query params and wire up presigning config
                {
                    let mut props = request.properties_mut();
                    props.insert(presigning_config.start_time());

                    let mut config = props.get_mut::<#{sig_auth}::signer::OperationSigningConfig>()
                        .expect("signing config added by make_operation()");
                    config.signature_type = #{sig_auth}::signer::HttpSignatureType::HttpRequestQueryParams(
                        #{sig_auth}::signer::HttpBodySigningType::$payloadSigningType
                    );
                    config.expires_in = Some(presigning_config.expires());
                }

                let middleware = #{aws_hyper}::AwsMiddleware::default();
                let mut svc = #{tower}::builder::ServiceBuilder::new()
                    .layer(&middleware)
                    .service(#{PresignedRequestService}::new());

                use #{tower}::{Service, ServiceExt};
                Ok(svc.ready().await?.call(request).await?)
                """,
                *codegenScope
            )
        }
    }
}

class AwsPresignedFluentBuilderMethod(
    runtimeConfig: RuntimeConfig,
) : FluentClientCustomization() {
    private val codegenScope = arrayOf(
        "Error" to AwsRuntimeType.Presigning.member("config::Error"),
        "PresignedRequest" to AwsRuntimeType.Presigning.member("request::PresignedRequest"),
        "PresigningConfig" to AwsRuntimeType.Presigning.member("config::PresigningConfig"),
        "SdkError" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("result::SdkError")
    )

    override fun section(section: FluentClientSection): Writable = writable {
        if (section is FluentClientSection.FluentBuilderImpl && section.operationShape.hasTrait(PresignableTrait::class.java)) {
            rustBlockTemplate(
                """
                pub async fn presigned(
                    self,
                    presigning_config: #{PresigningConfig},
                ) -> Result<#{PresignedRequest}, #{SdkError}<#{OpError}>>
                """,
                *codegenScope,
                "OpError" to section.operationErrorType
            ) {
                rustTemplate(
                    """
                    let input = self.inner.build().map_err(|err| #{SdkError}::ConstructionFailure(err.into()))?;
                    input.presigned(&self.handle.conf, presigning_config).await
                    """,
                    *codegenScope
                )
            }
        }
    }
}

interface PresignModelTransform {
    fun transform(model: Model): Model
}

class AwsPollySynthesizeSpeechPresignTransform : PresignModelTransform {
    private val synthesizeSpeechOpId = ShapeId.from("com.amazonaws.polly#SynthesizeSpeech")
    private val presignableOperations = listOf(synthesizeSpeechOpId)
    private val httpMethodOverrides = mapOf(synthesizeSpeechOpId to "GET")

    override fun transform(model: Model): Model {
        val index = HttpBindingIndex(model)

        // Find all known presignable operations
        val operationsToUpdate: MutableSet<Shape> = model.shapes()
            .filter { shape -> shape is OperationShape && presignableOperations.contains(shape.id) }
            .collect(Collectors.toSet())

        // Find document members of those presignable operations
        val membersToUpdate = operationsToUpdate.map { shape ->
            val operation = shape as OperationShape
            val payloadBindings = index.getRequestBindings(operation, HttpBinding.Location.DOCUMENT)
            payloadBindings.map { binding -> binding.member }
        }.flatten()

        // Transform found shapes for presigning
        val shapesToUpdate = operationsToUpdate + membersToUpdate
        return ModelTransformer.create().mapShapes(model) { shape -> mapShape(shapesToUpdate, shape) }
    }

    private fun mapShape(shapesToUpdate: Set<Shape>, shape: Shape): Shape {
        if (shapesToUpdate.contains(shape)) {
            if (shape is OperationShape && httpMethodOverrides.containsKey(shape.id)) {
                val newMethod = httpMethodOverrides.getValue(shape.id)
                val originalHttpTrait = shape.expectTrait<HttpTrait>()
                return shape.toBuilder()
                    .removeTrait(HttpTrait.ID)
                    .addTrait(originalHttpTrait.toBuilder().method(newMethod).build())
                    .build()
            } else if (shape is MemberShape) {
                return shape.toBuilder().addTrait(HttpQueryTrait(shape.memberName)).build()
            }
        }
        return shape
    }
}
