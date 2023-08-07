/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

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

/** Only apply this decorator to the given service ID */
fun ClientCodegenDecorator.onlyApplyTo(serviceId: String): List<ClientCodegenDecorator> =
    listOf(ServiceSpecificDecorator(ShapeId.from(serviceId), this))

/** Apply the given decorators only to this service ID */
fun String.applyDecorators(vararg decorators: ClientCodegenDecorator): List<ClientCodegenDecorator> =
    decorators.map { it.onlyApplyTo(this) }.flatten()

/**
 * Delegating decorator that only applies to a configured service ID
 */
class ServiceSpecificDecorator(
    /** Service ID this decorator is active for */
    private val appliesToServiceId: ShapeId,
    /** Decorator to delegate to */
    private val delegateTo: ClientCodegenDecorator,
    /** Decorator name */
    override val name: String = "${appliesToServiceId.namespace}.${appliesToServiceId.name}",
    /** Decorator order */
    override val order: Byte = 0,
) : ClientCodegenDecorator {
    private fun <T> T.maybeApply(serviceId: ToShapeId, delegatedValue: () -> T): T =
        if (appliesToServiceId == serviceId.toShapeId()) {
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
    ): List<AuthSchemeOption> = baseAuthSchemeOptions.maybeApply(codegenContext.serviceShape) {
        delegateTo.authOptions(codegenContext, operationShape, baseAuthSchemeOptions)
    }

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.builderCustomizations(codegenContext, baseCustomizations)
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.configCustomizations(codegenContext, baseCustomizations)
    }

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations =
        emptyMap<String, Any?>().maybeApply(codegenContext.serviceShape) {
            delegateTo.crateManifestCustomizations(codegenContext)
        }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
        emptyList<EndpointCustomization>().maybeApply(codegenContext.serviceShape) {
            delegateTo.endpointCustomizations(codegenContext)
        }

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.errorCustomizations(codegenContext, baseCustomizations)
    }

    override fun errorImplCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorImplCustomization>,
    ): List<ErrorImplCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.errorImplCustomizations(codegenContext, baseCustomizations)
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        maybeApply(codegenContext.serviceShape) {
            delegateTo.extras(codegenContext, rustCrate)
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.libRsCustomizations(codegenContext, baseCustomizations)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.operationCustomizations(codegenContext, operation, baseCustomizations)
    }

    override fun protocols(serviceId: ShapeId, currentProtocols: ClientProtocolMap): ClientProtocolMap =
        currentProtocols.maybeApply(serviceId) {
            delegateTo.protocols(serviceId, currentProtocols)
        }

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.structureCustomizations(codegenContext, baseCustomizations)
    }

    override fun transformModel(service: ServiceShape, model: Model, settings: ClientRustSettings): Model =
        model.maybeApply(service) {
            delegateTo.transformModel(service, model, settings)
        }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations.maybeApply(codegenContext.serviceShape) {
        delegateTo.serviceRuntimePluginCustomizations(codegenContext, baseCustomizations)
    }

    override fun protocolTestGenerator(
        codegenContext: ClientCodegenContext,
        baseGenerator: ProtocolTestGenerator,
    ): ProtocolTestGenerator = baseGenerator.maybeApply(codegenContext.serviceShape) {
        delegateTo.protocolTestGenerator(codegenContext, baseGenerator)
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf<AdHocCustomization>().maybeApply(codegenContext.serviceShape) {
            delegateTo.extraSections(codegenContext)
        }
}
