/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthCustomization
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ErrorCustomization
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.customize.CombinedCoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.customize.CoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import java.util.ServiceLoader
import java.util.logging.Logger

typealias ClientProtocolMap = ProtocolMap<OperationGenerator, ClientCodegenContext>

/**
 * [ClientCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer may wish to add a different set of derive
 * attributes to the generated classes.
 */
interface ClientCodegenDecorator : CoreCodegenDecorator<ClientCodegenContext, ClientRustSettings> {
    fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> = baseAuthSchemeOptions

    fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations

    fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations

    /**
     * Hook to customize generated errors.
     */
    fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> = baseCustomizations

    fun protocols(
        serviceId: ShapeId,
        currentProtocols: ClientProtocolMap,
    ): ClientProtocolMap = currentProtocols

    fun authCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AuthCustomization>,
    ): List<AuthCustomization> = baseCustomizations

    fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> = listOf()

    /**
     * Hook to customize client construction documentation.
     */
    fun clientConstructionDocs(
        codegenContext: ClientCodegenContext,
        baseDocs: Writable,
    ): Writable = baseDocs

    /**
     * Hooks to register additional service-level runtime plugins at codegen time
     */
    fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations
}

/**
 * [CombinedClientCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
open class CombinedClientCodegenDecorator(decorators: List<ClientCodegenDecorator>) :
    CombinedCoreCodegenDecorator<ClientCodegenContext, ClientRustSettings, ClientCodegenDecorator>(decorators), ClientCodegenDecorator {
    override val name: String
        get() = "CombinedClientCodegenDecorator"
    override val order: Byte
        get() = 0

    override fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> =
        combineCustomizations(baseAuthSchemeOptions) { decorator, authOptions ->
            decorator.authSchemeOptions(codegenContext, authOptions)
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.configCustomizations(codegenContext, customizations)
        }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.operationCustomizations(codegenContext, operation, customizations)
        }

    override fun errorCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ErrorCustomization>,
    ): List<ErrorCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.errorCustomizations(codegenContext, customizations)
        }

    override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ClientProtocolMap,
    ): ClientProtocolMap =
        combineCustomizations(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }

    override fun authCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AuthCustomization>,
    ): List<AuthCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.authCustomizations(codegenContext, customizations)
        }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
        addCustomizations { decorator -> decorator.endpointCustomizations(codegenContext) }

    override fun clientConstructionDocs(
        codegenContext: ClientCodegenContext,
        baseDocs: Writable,
    ): Writable =
        combineCustomizations(baseDocs) { decorator, customizations ->
            decorator.clientConstructionDocs(codegenContext, customizations)
        }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.serviceRuntimePluginCustomizations(codegenContext, customizations)
        }

    companion object {
        fun fromClasspath(
            context: PluginContext,
            vararg extras: ClientCodegenDecorator,
            logger: Logger = Logger.getLogger("RustClientCodegenSPILoader"),
        ): CombinedClientCodegenDecorator {
            val decorators =
                ServiceLoader.load(
                    ClientCodegenDecorator::class.java,
                    context.pluginClassLoader.orElse(ClientCodegenDecorator::class.java.classLoader),
                )

            val filteredDecorators =
                decorators.asSequence()
                    .onEach { logger.info("Discovered Codegen Decorator: ${it.javaClass.name}") }
                    .filter { it.classpathDiscoverable() }
                    .onEach { logger.info("Adding Codegen Decorator: ${it.javaClass.name}") }
                    .toList()
            return CombinedClientCodegenDecorator(filteredDecorators + extras)
        }
    }
}
