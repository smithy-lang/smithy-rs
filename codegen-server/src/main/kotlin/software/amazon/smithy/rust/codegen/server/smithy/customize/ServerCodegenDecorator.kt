/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.customize.CombinedCoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.customize.CoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.ValidationResult
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConfigMethod
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import java.util.logging.Logger

typealias ServerProtocolMap = ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>

/**
 * [ServerCodegenDecorator] allows downstream users to customize code generation.
 */
interface ServerCodegenDecorator : CoreCodegenDecorator<ServerCodegenContext, ServerRustSettings> {
    fun protocols(
        serviceId: ShapeId,
        currentProtocols: ServerProtocolMap,
    ): ServerProtocolMap = currentProtocols

    fun validationExceptionConversion(codegenContext: ServerCodegenContext): ValidationExceptionConversionGenerator? =
        null

    /**
     * Injection point to allow a decorator to postprocess the error message that arises when an operation is
     * constrained but the `ValidationException` shape is not attached to the operation's errors.
     */
    fun postprocessValidationExceptionNotAttachedErrorMessage(validationResult: ValidationResult) = validationResult

    /**
     * Injection point to allow a decorator to postprocess the error message that arises when an operation is
     * constrained and there are multiple of: `ValidationException` and/or custom structures with the
     * @validationException trait attached to the operation's errors.
     */
    fun postprocessMultipleValidationExceptionsErrorMessage(validationResult: ValidationResult) = validationResult

    /**
     * For each operation in the service's closure, this hook allows decorators to return a collection of structure shapes that will additionally be generated.
     * If a structure shape is in the service's closure, note that returning it here will cause for it to be generated more than once,
     * making the resulting crate not compile (since it will contain more than one struct with the same name).
     * Therefore, ensure that all the structure shapes returned by this method are not in the service's closure.
     */
    fun postprocessOperationGenerateAdditionalStructures(operationShape: OperationShape): List<StructureShape> =
        emptyList()

    /**
     * For each service, this hook allows decorators to return a collection of structure shapes that will additionally be generated.
     * If a structure shape is in the service's closure, note that returning it here will cause for it to be generated more than once,
     * making the resulting crate not compile (since it will contain more than one struct with the same name).
     * Therefore, ensure that all the structure shapes returned by this method are not in the service's closure.
     */
    fun postprocessServiceGenerateAdditionalStructures(serviceShape: ServiceShape): List<StructureShape> = emptyList()

    /**
     * Configuration methods that should be injected into the `${serviceName}Config` struct to allow users to configure
     * pre-applied layers and plugins.
     */
    fun configMethods(codegenContext: ServerCodegenContext): List<ConfigMethod> = emptyList()
}

/**
 * [CombinedServerCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
class CombinedServerCodegenDecorator(decorators: List<ServerCodegenDecorator>) :
    CombinedCoreCodegenDecorator<ServerCodegenContext, ServerRustSettings, ServerCodegenDecorator>(decorators),
    ServerCodegenDecorator {
    private val orderedDecorators = decorators.sortedBy { it.order }

    override val name: String
        get() = "CombinedServerCodegenDecorator"
    override val order: Byte
        get() = 0

    override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ServerProtocolMap,
    ): ServerProtocolMap =
        combineCustomizations(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator =
        // We use `firstNotNullOf` instead of `firstNotNullOfOrNull` because the [SmithyValidationExceptionDecorator]
        // is registered.
        orderedDecorators.firstNotNullOf { it.validationExceptionConversion(codegenContext) }

    override fun postprocessValidationExceptionNotAttachedErrorMessage(
        validationResult: ValidationResult,
    ): ValidationResult =
        orderedDecorators.foldRight(validationResult) { decorator, accumulated ->
            decorator.postprocessValidationExceptionNotAttachedErrorMessage(accumulated)
        }

    override fun postprocessMultipleValidationExceptionsErrorMessage(
        validationResult: ValidationResult,
    ): ValidationResult =
        orderedDecorators.foldRight(validationResult) { decorator, accumulated ->
            decorator.postprocessMultipleValidationExceptionsErrorMessage(accumulated)
        }

    override fun postprocessOperationGenerateAdditionalStructures(
        operationShape: OperationShape,
    ): List<StructureShape> =
        orderedDecorators.flatMap { it.postprocessOperationGenerateAdditionalStructures(operationShape) }

    override fun postprocessServiceGenerateAdditionalStructures(serviceShape: ServiceShape): List<StructureShape> =
        orderedDecorators.flatMap { it.postprocessServiceGenerateAdditionalStructures(serviceShape) }

    override fun configMethods(codegenContext: ServerCodegenContext): List<ConfigMethod> =
        orderedDecorators.flatMap { it.configMethods(codegenContext) }

    companion object {
        fun fromClasspath(
            context: PluginContext,
            vararg extras: ServerCodegenDecorator,
            logger: Logger = Logger.getLogger("RustServerCodegenSPILoader"),
        ): CombinedServerCodegenDecorator {
            val decorators = decoratorsFromClasspath(context, ServerCodegenDecorator::class.java, logger, *extras)
            return CombinedServerCodegenDecorator(decorators)
        }
    }
}
