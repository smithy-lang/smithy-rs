/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customizations.AuthEndpointOrchestrationV2MarkerCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ConnectionPoisoningRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpChecksumRequiredGenerator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.IdentityCacheConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.InterceptorConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.MetadataCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.RequestCompressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ResiliencyConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ResiliencyReExportCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.RetryClassifierConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.RetryClassifierOperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.RetryClassifierServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.RetryModeFeatureTrackerRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customizations.TimeSourceCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.AllowLintsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.pubUseSmithyPrimitives
import software.amazon.smithy.rust.codegen.core.smithy.customizations.pubUseSmithyPrimitivesEventStream
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError

val TestUtilFeature = Feature("test-util", false, listOf())

/**
 * A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 */
class RequiredCustomizations : ClientCodegenDecorator {
    override val name: String = "Required"
    override val order: Byte = -1

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations +
            MetadataCustomization(codegenContext, operation) +
            HttpChecksumRequiredGenerator(codegenContext, operation) +
            RetryClassifierOperationCustomization(codegenContext, operation) +
            RequestCompressionGenerator(codegenContext, operation)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations +
            ResiliencyConfigCustomization(codegenContext) +
            IdentityCacheConfigCustomization(codegenContext) +
            InterceptorConfigCustomization(codegenContext) +
            TimeSourceCustomization(codegenContext) +
            RetryClassifierConfigCustomization(codegenContext)

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations + AllowLintsCustomization()

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig

        // Add rt-tokio feature for `ByteStream::from_path`
        rustCrate.mergeFeature(
            Feature(
                "rt-tokio",
                true,
                listOf("aws-smithy-async/rt-tokio", "aws-smithy-types/rt-tokio"),
            ),
        )

        rustCrate.mergeFeature(TestUtilFeature)

        // Re-export resiliency types
        ResiliencyReExportCustomization(codegenContext).extras(rustCrate)

        rustCrate.withModule(ClientRustModule.primitives) {
            pubUseSmithyPrimitives(codegenContext, codegenContext.model, rustCrate)(this)
        }
        rustCrate.withModule(ClientRustModule.Primitives.EventStream) {
            pubUseSmithyPrimitivesEventStream(codegenContext, codegenContext.model)(this)
        }
        rustCrate.withModule(ClientRustModule.Error) {
            rustTemplate(
                """
                /// Error type returned by the client.
                pub type SdkError<E, R = #{R}> = #{SdkError}<E, R>;
                pub use #{BuildError};
                pub use #{ConnectorError};

                pub use #{DisplayErrorContext};
                pub use #{ProvideErrorMetadata};
                pub use #{ErrorMetadata};
                """,
                "DisplayErrorContext" to RuntimeType.smithyTypes(rc).resolve("error::display::DisplayErrorContext"),
                "ProvideErrorMetadata" to RuntimeType.smithyTypes(rc).resolve("error::metadata::ProvideErrorMetadata"),
                "ErrorMetadata" to RuntimeType.smithyTypes(rc).resolve("error::metadata::ErrorMetadata"),
                "R" to RuntimeType.smithyRuntimeApiClient(rc).resolve("client::orchestrator::HttpResponse"),
                "SdkError" to RuntimeType.sdkError(rc),
                // this can't use the auto-rexport because the builder generator is defined in codegen core
                "BuildError" to rc.operationBuildError(),
                "ConnectorError" to RuntimeType.smithyRuntimeApi(rc).resolve("client::result::ConnectorError"),
            )
        }

        ClientRustModule.Meta.also { metaModule ->
            rustCrate.withModule(metaModule) {
                CrateVersionCustomization.extras(rustCrate, metaModule)
            }
        }
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations +
            ConnectionPoisoningRuntimePluginCustomization(codegenContext) +
            RetryClassifierServiceRuntimePluginCustomization(codegenContext) +
            RetryModeFeatureTrackerRuntimePluginCustomization(codegenContext) +
            AuthEndpointOrchestrationV2MarkerCustomization(codegenContext)
}
