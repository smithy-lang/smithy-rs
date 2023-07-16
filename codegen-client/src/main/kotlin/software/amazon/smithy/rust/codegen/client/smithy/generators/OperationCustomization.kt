/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

sealed class OperationSection(name: String) : Section(name) {
    abstract val customizations: List<OperationCustomization>

    /** Write custom code into the `impl` block of this operation */
    data class OperationImplBlock(override val customizations: List<OperationCustomization>) :
        OperationSection("OperationImplBlock")

    // TODO(enableNewSmithyRuntimeCleanup): Delete this customization hook when cleaning up middleware
    /** Write additional functions inside the Input's impl block */
    @Deprecated("customization for middleware; won't be used in the orchestrator impl")
    data class InputImpl(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
        val inputShape: StructureShape,
        val protocol: Protocol,
    ) : OperationSection("InputImpl")

    // TODO(enableNewSmithyRuntimeCleanup): Delete this customization hook when cleaning up middleware
    @Deprecated("customization for middleware; won't be used in the orchestrator impl")
    data class MutateInput(
        override val customizations: List<OperationCustomization>,
        val input: String,
        val config: String,
    ) : OperationSection("MutateInput")

    // TODO(enableNewSmithyRuntimeCleanup): Delete this customization hook when cleaning up middleware
    /** Write custom code into the block that builds an operation
     *
     * [request]: Name of the variable holding the `aws_smithy_http::Request`
     * [config]: Name of the variable holding the service config.
     *
     * */
    @Deprecated("customization for middleware; won't be used in the orchestrator impl")
    data class MutateRequest(
        override val customizations: List<OperationCustomization>,
        val request: String,
        val config: String,
    ) : OperationSection("Feature")

    // TODO(enableNewSmithyRuntimeCleanup): Delete this customization hook when cleaning up middleware
    @Deprecated("customization for middleware; won't be used in the orchestrator impl")
    data class FinalizeOperation(
        override val customizations: List<OperationCustomization>,
        val operation: String,
        val config: String,
    ) : OperationSection("Finalize")

    data class MutateOutput(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
        /** Name of the response headers map (for referring to it in Rust code) */
        val responseHeadersName: String,

        // TODO(enableNewSmithyRuntimeCleanup): Remove this flag when switching to the orchestrator
        /** Whether the property bag exists in this context */
        val propertyBagAvailable: Boolean,
    ) : OperationSection("MutateOutput")

    /**
     * Allows for adding additional properties to the `extras` field on the
     * `aws_smithy_types::error::ErrorMetadata`.
     */
    data class PopulateErrorMetadataExtras(
        override val customizations: List<OperationCustomization>,
        /** Name of the generic error builder (for referring to it in Rust code) */
        val builderName: String,
        /** Name of the response status (for referring to it in Rust code) */
        val responseStatusName: String,
        /** Name of the response headers map (for referring to it in Rust code) */
        val responseHeadersName: String,
    ) : OperationSection("PopulateErrorMetadataExtras")

    /**
     * Hook to add custom code right before the response is parsed.
     */
    data class BeforeParseResponse(
        override val customizations: List<OperationCustomization>,
        val responseName: String,
    ) : OperationSection("BeforeParseResponse")

    /**
     * Hook for adding additional things to config inside operation runtime plugins.
     */
    data class AdditionalRuntimePluginConfig(
        override val customizations: List<OperationCustomization>,
        val newLayerName: String,
        val operationShape: OperationShape,
    ) : OperationSection("AdditionalRuntimePluginConfig")

    data class AdditionalInterceptors(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
    ) : OperationSection("AdditionalInterceptors") {
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
            writer.rustTemplate(
                """
                .with_interceptor(
                    #{SharedInterceptor}::new(
                        #{interceptor}
                    ) as _
                )
                """,
                "interceptor" to interceptor,
                "SharedInterceptor" to smithyRuntimeApi.resolve("client::interceptors::SharedInterceptor"),
            )
        }
    }

    /**
     * Hook for adding retry classifiers to an operation's `RetryClassifiers` bundle.
     *
     * Should emit 1+ lines of code that look like the following:
     * ```rust
     * .with_classifier(AwsErrorCodeClassifier::new())
     * .with_classifier(HttpStatusCodeClassifier::new())
     * ```
     */
    data class RetryClassifier(
        override val customizations: List<OperationCustomization>,
        val configBagName: String,
        val operationShape: OperationShape,
    ) : OperationSection("RetryClassifier")

    /**
     * Hook for adding supporting types for operation-specific runtime plugins.
     * Examples include various operation-specific types (retry classifiers, config bag types, etc.)
     */
    data class RuntimePluginSupportingTypes(
        override val customizations: List<OperationCustomization>,
        val configBagName: String,
        val operationShape: OperationShape,
    ) : OperationSection("RuntimePluginSupportingTypes")

    /**
     * Hook for adding additional runtime plugins to an operation.
     */
    data class AdditionalRuntimePlugins(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
    ) : OperationSection("AdditionalRuntimePlugins") {
        fun addServiceRuntimePlugin(writer: RustWriter, plugin: Writable) {
            writer.rustTemplate(".with_service_plugin(#{plugin})", "plugin" to plugin)
        }

        fun addOperationRuntimePlugin(writer: RustWriter, plugin: Writable) {
            writer.rustTemplate(".with_operation_plugin(#{plugin})", "plugin" to plugin)
        }
    }
}

abstract class OperationCustomization : NamedCustomization<OperationSection>() {
    // TODO(enableNewSmithyRuntimeCleanup): Delete this when cleaning up middleware
    @Deprecated("property for middleware; won't be used in the orchestrator impl")
    open fun retryType(): RuntimeType? = null

    // TODO(enableNewSmithyRuntimeCleanup): Delete this when cleaning up middleware
    /**
     * Does `make_operation` consume the self parameter?
     *
     * This is required for things like idempotency tokens where the operation can only be sent once
     * and an idempotency token will mutate the request.
     */
    @Deprecated("property for middleware; won't be used in the orchestrator impl")
    open fun consumesSelf(): Boolean = false

    // TODO(enableNewSmithyRuntimeCleanup): Delete this when cleaning up middleware
    /**
     * Does `make_operation` mutate the self parameter?
     */
    @Deprecated("property for middleware; won't be used in the orchestrator impl")
    open fun mutSelf(): Boolean = false
}
