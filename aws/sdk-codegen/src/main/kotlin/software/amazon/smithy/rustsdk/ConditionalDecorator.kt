/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientProtocolMap
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplCustomization

/**
 * Delegating decorator that only applies when a condition is true
 */
class ConditionalDecorator(
    /** Decorator to delegate to */
    private val delegateTo: ClientCodegenDecorator,
    /** Service ID this decorator is active for */
    private val predicate: (ClientCodegenContext?, ShapeId?) -> Boolean,
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

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> =
        baseAuthSchemeOptions.maybeApply(codegenContext) {
            delegateTo.authOptions(codegenContext, operationShape, baseAuthSchemeOptions)
        }

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.builderCustomizations(codegenContext, baseCustomizations)
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.configCustomizations(codegenContext, baseCustomizations)
        }

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations =
        emptyMap<String, Any?>().maybeApply(codegenContext) {
            delegateTo.crateManifestCustomizations(codegenContext)
        }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
        emptyList<EndpointCustomization>().maybeApply(codegenContext) {
            delegateTo.endpointCustomizations(codegenContext)
        }

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.errorCustomizations(codegenContext, baseCustomizations)
        }

    override fun errorImplCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorImplCustomization>,
    ): List<ErrorImplCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.errorImplCustomizations(codegenContext, baseCustomizations)
        }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        maybeApply(codegenContext) {
            delegateTo.extras(codegenContext, rustCrate)
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.libRsCustomizations(codegenContext, baseCustomizations)
        }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.operationCustomizations(codegenContext, operation, baseCustomizations)
        }

    override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ClientProtocolMap,
    ): ClientProtocolMap =
        currentProtocols.maybeApply(serviceShapeId = serviceId) {
            delegateTo.protocols(serviceId, currentProtocols)
        }

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.structureCustomizations(codegenContext, baseCustomizations)
        }

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        model.maybeApply(serviceShapeId = service) {
            delegateTo.transformModel(service, model, settings)
        }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.maybeApply(codegenContext) {
            delegateTo.serviceRuntimePluginCustomizations(codegenContext, baseCustomizations)
        }

    override fun protocolTestGenerator(
        codegenContext: ClientCodegenContext,
        baseGenerator: ProtocolTestGenerator,
    ): ProtocolTestGenerator =
        baseGenerator.maybeApply(codegenContext) {
            delegateTo.protocolTestGenerator(codegenContext, baseGenerator)
        }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf<AdHocCustomization>().maybeApply(codegenContext) {
            delegateTo.extraSections(codegenContext)
        }
}
