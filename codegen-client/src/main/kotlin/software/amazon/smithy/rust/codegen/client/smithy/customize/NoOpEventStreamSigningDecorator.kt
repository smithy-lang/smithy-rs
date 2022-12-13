/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.EventStreamSigningConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations

/**
 * The NoOpEventStreamSigningDecorator:
 * - adds a `new_event_stream_signer()` method to `config` to create an Event Stream NoOp signer
 */
open class NoOpEventStreamSigningDecorator<T, C : CodegenContext> : RustCodegenDecorator<T, C> {
    override val name: String = "NoOpEventStreamSigning"
    override val order: Byte = Byte.MIN_VALUE

    private fun applies(codegenContext: CodegenContext, baseCustomizations: List<ConfigCustomization>): Boolean =
        codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model) &&
            // and if there is no other `EventStreamSigningConfig`, apply this one
            !baseCustomizations.any { it is EventStreamSigningConfig }

    override fun configCustomizations(
        codegenContext: C,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        if (!applies(codegenContext, baseCustomizations)) {
            return baseCustomizations
        }
        return baseCustomizations + NoOpEventStreamSigningConfig(
            codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model),
            codegenContext.runtimeConfig,
        )
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>) = true
}

class NoOpEventStreamSigningConfig(
    private val serviceHasEventStream: Boolean,
    runtimeConfig: RuntimeConfig,
) : EventStreamSigningConfig(runtimeConfig) {

    private val codegenScope = arrayOf(
        "NoOpSigner" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::NoOpSigner"),
    )

    override fun configImplSection() = renderEventStreamSignerFn {
        writable {
            if (serviceHasEventStream) {
                rustTemplate(
                    """
                    #{NoOpSigner}{}
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
