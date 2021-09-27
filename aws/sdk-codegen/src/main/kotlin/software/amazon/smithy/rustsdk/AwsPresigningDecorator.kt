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
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rustsdk.traits.PresignableTrait

private val PRESIGNABLE_OPERATIONS = listOf(
    // TODO(PresignedRequests): Add the other presignable operations
    ShapeId.from("com.amazonaws.s3#GetObject"),
    ShapeId.from("com.amazonaws.s3#PutObject"),
)

class AwsPresigningDecorator : RustCodegenDecorator {
    companion object {
        const val ORDER: Byte = 0
    }

    override val name: String = "AwsPresigning"
    override val order: Byte = ORDER

    override fun extras(rustSettings: RustSettings, protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        val hasPresignedOps = protocolConfig.model.shapes().anyMatch { shape ->
            shape is OperationShape && PRESIGNABLE_OPERATIONS.contains(shape.id)
        }
        if (hasPresignedOps) {
            rustCrate.mergeFeature(Feature("client", default = true, listOf("tower")))
        }
    }

    override fun operationCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> = baseCustomizations + listOf(
        AwsInputPresignedMethod(protocolConfig.runtimeConfig, protocolConfig.symbolProvider, operation)
    )

    /** Adds presignable trait to known presignable operations */
    override fun transformModel(rustSettings: RustSettings, service: ServiceShape, model: Model): Model {
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
        if (section is OperationSection.InputImpl && section.operationShape.hasTrait(PresignableTrait::class.java)) {
            writeInputPresignedMethod()
        }
    }

    private fun RustWriter.writeInputPresignedMethod() {
        val operationError = operationShape.errorSymbol(symbolProvider)
        rustBlockTemplate(
            """
            /// Creates a presigned request for this operation. The credentials provider from the `config`
            /// will be used to generate the request's signature, and the `presignining_config` provides additional
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
                let (mut request, _) = self.make_operation(config)
                    .map_err(|err| #{SdkError}::ConstructionFailure(err.into()))?
                    .into_request_response();

                // Change signature type to query params and wire up presigning config
                {
                    let mut props = request.properties_mut();
                    props.insert(presigning_config.start_time());

                    let mut config = props.get_mut::<#{sig_auth}::signer::OperationSigningConfig>()
                        .expect("signing config added by make_operation()");
                    config.signature_type = #{sig_auth}::signer::HttpSignatureType::HttpRequestQueryParams;
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
