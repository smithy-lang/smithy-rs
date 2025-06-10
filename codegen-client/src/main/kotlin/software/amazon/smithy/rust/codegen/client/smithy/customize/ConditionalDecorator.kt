/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthCustomization
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator

/**
 * Delegating decorator that only applies when a condition is true
 */
open class ConditionalDecorator(
    /** Decorator to delegate to */
    private val delegateTo: ClientCodegenDecorator,
    private val predicate: (ClientCodegenContext?, ToShapeId?) -> Boolean,
) : ClientCodegenDecorator {
    override val name: String = delegateTo.name
    override val order: Byte = delegateTo.order

    private fun <T> T.maybeApply(
        codegenContext: ClientCodegenContext? = null,
        serviceShapeId: ToShapeId? = null,
        delegatedValue: () -> T,
    ): T =
        if (predicate(codegenContext, (serviceShapeId ?: codegenContext?.serviceShape)?.toShapeId())) {
            delegatedValue()
        } else {
            this
        }

    // This kind of decorator gets explicitly added to the root sdk-codegen decorator
    override fun classpathDiscoverable(): Boolean = false

    final override fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> =
        baseAuthSchemeOptions.maybeApply(codegenContext) {
            delegateTo.authSchemeOptions(codegenContext, baseAuthSchemeOptions)
        }

    final override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.builderCustomizations(codegenContext, baseCustomizations)
        }

    final override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.configCustomizations(codegenContext, baseCustomizations)
        }

    final override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations =
        emptyMap<String, Any?>().maybeApply(codegenContext) {
            delegateTo.crateManifestCustomizations(codegenContext)
        }

    final override fun authCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AuthCustomization>,
    ): List<AuthCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.authCustomizations(codegenContext, baseCustomizations)
        }

    final override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
        emptyList<EndpointCustomization>().maybeApply(codegenContext) {
            delegateTo.endpointCustomizations(codegenContext)
        }

    final override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.errorCustomizations(codegenContext, baseCustomizations)
        }

    final override fun errorImplCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorImplCustomization>,
    ): List<ErrorImplCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.errorImplCustomizations(codegenContext, baseCustomizations)
        }

    final override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        maybeApply(codegenContext) {
            delegateTo.extras(codegenContext, rustCrate)
        }
    }

    final override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.libRsCustomizations(codegenContext, baseCustomizations)
        }

    final override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.operationCustomizations(codegenContext, operation, baseCustomizations)
        }

    final override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ClientProtocolMap,
    ): ClientProtocolMap =
        currentProtocols.maybeApply(serviceShapeId = serviceId) {
            delegateTo.protocols(serviceId, currentProtocols)
        }

    final override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.structureCustomizations(codegenContext, baseCustomizations)
        }

    final override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        model.maybeApply(serviceShapeId = service) {
            delegateTo.transformModel(service, model, settings)
        }

    final override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.serviceRuntimePluginCustomizations(codegenContext, baseCustomizations)
        }

    final override fun protocolTestGenerator(
        codegenContext: ClientCodegenContext,
        baseGenerator: ProtocolTestGenerator,
    ): ProtocolTestGenerator =
        baseGenerator.maybeApply(codegenContext) {
            delegateTo.protocolTestGenerator(codegenContext, baseGenerator)
        }

    final override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf<AdHocCustomization>().maybeApply(codegenContext) {
            delegateTo.extraSections(codegenContext)
        }
}
