/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpQueryTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.InternalTraitsModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.fluentBuilderType
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.RequestSerializerGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.cloneOperation
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf
import software.amazon.smithy.rustsdk.traits.PresignableTrait

private val presigningTypes: Array<Pair<String, Any>> =
    arrayOf(
        "PresignedRequest" to AwsRuntimeType.presigning().resolve("PresignedRequest"),
        "PresigningConfig" to AwsRuntimeType.presigning().resolve("PresigningConfig"),
    )

internal enum class PayloadSigningType {
    EMPTY,
    UNSIGNED_PAYLOAD,
}

private fun syntheticShapeId(shape: ToShapeId): ShapeId =
    shape.toShapeId().let { id -> ShapeId.fromParts(id.namespace + ".synthetic.aws.presigned", id.name) }

internal class PresignableOperation(
    val payloadSigningType: PayloadSigningType,
    val modelTransforms: List<PresignModelTransform> = emptyList(),
) {
    fun hasModelTransforms(): Boolean = modelTransforms.isNotEmpty()
}

private val SYNTHESIZE_SPEECH_OP = ShapeId.from("com.amazonaws.polly#SynthesizeSpeech")
internal val PRESIGNABLE_OPERATIONS by lazy {
    mapOf(
        // S3
        // TODO(https://github.com/awslabs/aws-sdk-rust/issues/488) Technically, all S3 operations support presigning
        ShapeId.from("com.amazonaws.s3#HeadObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.s3#GetObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.s3#PutObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.s3#UploadPart") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        ShapeId.from("com.amazonaws.s3#DeleteObject") to PresignableOperation(PayloadSigningType.UNSIGNED_PAYLOAD),
        // Polly
        SYNTHESIZE_SPEECH_OP to
            PresignableOperation(
                PayloadSigningType.EMPTY,
                // Polly's SynthesizeSpeech operation has the HTTP method overridden to GET,
                // and the document members changed to query param members.
                modelTransforms =
                    listOf(
                        OverrideHttpMethodTransform(mapOf(SYNTHESIZE_SPEECH_OP to "GET")),
                        MoveDocumentMembersToQueryParamsTransform(listOf(SYNTHESIZE_SPEECH_OP)),
                    ),
            ),
    )
}

class AwsPresigningDecorator internal constructor(
    private val presignableOperations: Map<ShapeId, PresignableOperation> = PRESIGNABLE_OPERATIONS,
) : ClientCodegenDecorator {
    companion object {
        const val ORDER: Byte = 0
    }

    override val name: String = "AwsPresigning"
    override val order: Byte = ORDER

    /**
     * Adds presignable trait to known presignable operations and creates synthetic presignable shapes for codegen
     */
    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model {
        val modelWithSynthetics = addSyntheticOperations(model)
        val presignableTransforms = mutableListOf<PresignModelTransform>()
        val intermediate =
            ModelTransformer.create().mapShapes(modelWithSynthetics) { shape ->
                if (shape is OperationShape && presignableOperations.containsKey(shape.id)) {
                    presignableTransforms.addAll(presignableOperations.getValue(shape.id).modelTransforms)
                    shape.toBuilder().addTrait(PresignableTrait(syntheticShapeId(shape))).build()
                } else {
                    shape
                }
            }
        // Apply operation-specific model transformations
        return presignableTransforms.fold(intermediate) { m, t -> t.transform(m) }
    }

    private fun addSyntheticOperations(model: Model): Model {
        val presignableOps =
            model.shapes()
                .filter { shape -> shape is OperationShape && presignableOperations.containsKey(shape.id) }
                .toList()
        return model.toBuilder().also { builder ->
            for (op in presignableOps) {
                builder.cloneOperation(model, op, ::syntheticShapeId)
            }
        }.build()
    }

    private fun anyPresignedShapes(ctx: ClientCodegenContext) =
        TopDownIndex.of(ctx.model).getContainedOperations(ctx.serviceShape)
            .any { presignableOperations.containsKey(it.id) }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        anyPresignedShapes(codegenContext).thenSingletonListOf {
            adhocCustomization<CustomizableOperationSection.CustomizableOperationImpl> {
                rustTemplate(
                    """
                    /// Sends the request and returns the response.
                    ##[allow(unused_mut)]
                    pub async fn presigned(mut self, presigning_config: #{PresigningConfig}) -> #{Result}<#{PresignedRequest}, crate::error::SdkError<E>> where
                        E: std::error::Error + #{Send} + #{Sync} + 'static,
                        B: #{CustomizablePresigned}<E>
                    {
                        self.execute(move |sender, conf|sender.presign(conf, presigning_config)).await
                    }
                    """,
                    *preludeScope,
                    *presigningTypes,
                    "CustomizablePresigned" to customizablePresigned,
                )
            }
        }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (anyPresignedShapes(codegenContext)) {
            rustCrate.mergeFeature(
                Feature(
                    "http-1x",
                    default = false,
                    listOf("dep:http-body-1x", "aws-smithy-runtime-api/http-1x"),
                ),
            )
        }
    }

    private val customizablePresigned =
        RuntimeType.forInlineFun("CustomizablePresigned", InternalTraitsModule) {
            rustTemplate(
                """
                pub trait CustomizablePresigned<E>: #{Send} + #{Sync} {
                    fn presign(self, config_override: crate::config::Builder, presigning_config: #{PresigningConfig}) -> BoxFuture<SendResult<#{PresignedRequest}, E>>;
                }

                """,
                *preludeScope,
                *presigningTypes,
            )
        }
}

class AwsPresignedFluentBuilderMethod(
    private val codegenContext: ClientCodegenContext,
) : FluentClientCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            *presigningTypes,
            "Error" to AwsRuntimeType.presigning().resolve("config::Error"),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
        )

    override fun section(section: FluentClientSection): Writable =
        writable {
            if (section is FluentClientSection.FluentBuilderImpl && section.operationShape.hasTrait(PresignableTrait::class.java)) {
                documentPresignedMethod(hasConfigArg = false)
                rustBlockTemplate(
                    """
                    ##[allow(unused_mut)]
                    pub async fn presigned(
                        mut self,
                        presigning_config: #{PresigningConfig},
                    ) -> #{Result}<#{PresignedRequest}, #{SdkError}<#{OpError}, #{RawResponseType}>>
                    """,
                    *codegenScope,
                    "OpError" to section.operationErrorType,
                    "RawResponseType" to
                        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                            .resolve("client::orchestrator::HttpResponse"),
                ) {
                    renderPresignedMethodBody(section)
                    val builderName = section.operationShape.fluentBuilderType(codegenContext.symbolProvider).name
                    addDependency(implementPresignedTrait(section, builderName).dependency!!)
                }
            }
        }

    private fun implementPresignedTrait(
        section: FluentClientSection.FluentBuilderImpl,
        builderName: String,
    ): RuntimeType {
        return RuntimeType.forInlineFun(
            "TraitImplementation",
            codegenContext.symbolProvider.moduleForBuilder(section.operationShape),
        ) {
            rustTemplate(
                """
                impl
                    crate::client::customize::internal::CustomizablePresigned<
                        #{OperationError},
                    > for $builderName
                {
                    fn presign(
                        self,
                        config_override: crate::config::Builder,
                        presigning_config: #{PresigningConfig}
                    ) -> crate::client::customize::internal::BoxFuture<
                        crate::client::customize::internal::SendResult<
                            #{PresignedRequest},
                            #{OperationError},
                        >,
                    > {
                        #{Box}::pin(async move { self.config_override(config_override).presigned(presigning_config).await })
                    }
                }
                """,
                *codegenScope,
                "OperationError" to section.operationErrorType,
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
        }
    }

    private fun RustWriter.renderPresignedMethodBody(section: FluentClientSection.FluentBuilderImpl) {
        val presignableOp = PRESIGNABLE_OPERATIONS.getValue(section.operationShape.id)
        val operationShape =
            if (presignableOp.hasModelTransforms()) {
                codegenContext.model.expectShape(
                    syntheticShapeId(section.operationShape.id),
                    OperationShape::class.java,
                )
            } else {
                section.operationShape
            }

        rustTemplate(
            """
            #{alternate_presigning_serializer}

            let runtime_plugins = #{Operation}::operation_runtime_plugins(
                self.handle.runtime_plugins.clone(),
                &self.handle.conf,
                self.config_override,
            )
                .with_client_plugin(#{SigV4PresigningRuntimePlugin}::new(presigning_config, #{payload_override}))
                #{alternate_presigning_serializer_registration};

            let input = self.inner.build().map_err(#{SdkError}::construction_failure)?;
            let mut context = #{Operation}::orchestrate_with_stop_point(&runtime_plugins, input, #{StopPoint}::BeforeTransmit)
                .await
                .map_err(|err| {
                    err.map_service_error(|err| {
                        err.downcast::<#{OperationError}>().expect("correct error type")
                    })
                })?;
            let request = context.take_request().expect("request set before transmit");
            #{PresignedRequest}::new(request).map_err(#{SdkError}::construction_failure)
            """,
            *codegenScope,
            "Operation" to codegenContext.symbolProvider.toSymbol(section.operationShape),
            "OperationError" to section.operationErrorType,
            "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
            "SharedInterceptor" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::interceptors")
                    .resolve("SharedInterceptor"),
            "SigV4PresigningRuntimePlugin" to
                AwsRuntimeType.presigningInterceptor(runtimeConfig)
                    .resolve("SigV4PresigningRuntimePlugin"),
            "StopPoint" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::StopPoint"),
            "USER_AGENT" to CargoDependency.Http.toType().resolve("header::USER_AGENT"),
            "alternate_presigning_serializer" to
                writable {
                    if (presignableOp.hasModelTransforms()) {
                        val smithyTypes = RuntimeType.smithyTypes(codegenContext.runtimeConfig)
                        rustTemplate(
                            """
                            ##[derive(::std::fmt::Debug)]
                            struct AlternatePresigningSerializerRuntimePlugin;
                            impl #{RuntimePlugin} for AlternatePresigningSerializerRuntimePlugin {
                                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                                    let mut cfg = #{Layer}::new("presigning_serializer");
                                    cfg.store_put(#{SharedRequestSerializer}::new(#{AlternateSerializer}));
                                    #{Some}(cfg.freeze())
                                }
                            }
                            """,
                            *preludeScope,
                            "AlternateSerializer" to alternateSerializer(operationShape),
                            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
                            "Layer" to smithyTypes.resolve("config_bag::Layer"),
                            "RuntimePlugin" to RuntimeType.runtimePlugin(codegenContext.runtimeConfig),
                            "SharedRequestSerializer" to
                                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                                    .resolve("client::ser_de::SharedRequestSerializer"),
                        )
                    }
                },
            "alternate_presigning_serializer_registration" to
                writable {
                    if (presignableOp.hasModelTransforms()) {
                        rust(".with_operation_plugin(AlternatePresigningSerializerRuntimePlugin)")
                    }
                },
            "payload_override" to
                writable {
                    rustTemplate(
                        "#{aws_sigv4}::http_request::SignableBody::" +
                            when (presignableOp.payloadSigningType) {
                                PayloadSigningType.EMPTY -> "Bytes(b\"\")"
                                PayloadSigningType.UNSIGNED_PAYLOAD -> "UnsignedPayload"
                            },
                        "aws_sigv4" to AwsRuntimeType.awsSigv4(runtimeConfig),
                    )
                },
        )
    }

    private fun alternateSerializer(transformedOperationShape: OperationShape): RuntimeType =
        transformedOperationShape.contextName(codegenContext.serviceShape).replaceFirstChar {
            it.uppercase()
        }.let { baseName ->
            "${baseName}PresigningRequestSerializer".let { name ->
                RuntimeType.forInlineFun(
                    name,
                    codegenContext.symbolProvider.moduleForShape(transformedOperationShape),
                ) {
                    RequestSerializerGenerator(
                        codegenContext,
                        codegenContext.protocolImpl!!,
                        null,
                        nameOverride = name,
                    ).render(
                        this,
                        transformedOperationShape,
                    )
                }
            }
        }
}

interface PresignModelTransform {
    fun transform(model: Model): Model
}

/**
 * Model transform that overrides HTTP request methods for the given map of operations.
 *
 * Note: this doesn't work for non-REST protocols. The protocol generators will need to be refactored
 * to respect HTTP traits or synthetic equivalents if this is needed for AwsQuery, Ec2Query, or AwsJson.
 */
class OverrideHttpMethodTransform(
    httpMethodOverrides: Map<ShapeId, String>,
) : PresignModelTransform {
    private val overrides = httpMethodOverrides.mapKeys { entry -> syntheticShapeId(entry.key) }

    override fun transform(model: Model): Model {
        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && overrides.containsKey(shape.id)) {
                val newMethod = overrides.getValue(shape.id)
                check(shape.hasTrait(HttpTrait.ID)) {
                    "OverrideHttpMethodTransform can only be used with REST protocols"
                }
                val originalHttpTrait = shape.expectTrait<HttpTrait>()
                shape.toBuilder()
                    .removeTrait(HttpTrait.ID)
                    .addTrait(originalHttpTrait.toBuilder().method(newMethod).build())
                    .build()
            } else {
                shape
            }
        }
    }
}

/**
 * Model transform that moves document members into query parameters for the given list of operations.
 *
 * Note: this doesn't work for non-REST protocols. The protocol generators will need to be refactored
 * to respect HTTP traits or synthetic equivalents if this is needed for AwsQuery, Ec2Query, or AwsJson.
 */
class MoveDocumentMembersToQueryParamsTransform(
    private val presignableOperations: List<ShapeId>,
) : PresignModelTransform {
    override fun transform(model: Model): Model {
        val index = HttpBindingIndex(model)
        val operations =
            presignableOperations.map { id ->
                model.expectShape(syntheticShapeId(id), OperationShape::class.java).also { shape ->
                    check(shape.hasTrait(HttpTrait.ID)) {
                        "MoveDocumentMembersToQueryParamsTransform can only be used with REST protocols"
                    }
                }
            }

        // Find document members of the presignable operations
        val membersToUpdate =
            operations.map { operation ->
                val payloadBindings = index.getRequestBindings(operation, HttpBinding.Location.DOCUMENT)
                payloadBindings.map { binding -> binding.member }
            }.flatten()

        // Transform found shapes for presigning
        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is MemberShape && membersToUpdate.contains(shape)) {
                shape.toBuilder().addTrait(HttpQueryTrait(shape.memberName)).build()
            } else {
                shape
            }
        }
    }
}

private fun RustWriter.documentPresignedMethod(hasConfigArg: Boolean) {
    val configBlurb =
        if (hasConfigArg) {
            "The credentials provider from the `config` will be used to generate the request's signature.\n"
        } else {
            ""
        }
    docs(
        """
        Creates a presigned request for this operation.

        ${configBlurb}The `presigning_config` provides additional presigning-specific config values, such as the
        amount of time the request should be valid for after creation.

        Presigned requests can be given to other users or applications to access a resource or perform
        an operation without having access to the AWS security credentials.

        _Important:_ If you're using credentials that can expire, such as those from STS AssumeRole or SSO, then
        the presigned request can only be valid for as long as the credentials used to create it are.
        """,
    )
}
