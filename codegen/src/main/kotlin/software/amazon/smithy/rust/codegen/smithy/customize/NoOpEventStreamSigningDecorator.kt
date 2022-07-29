/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.EventStreamSigningConfig
import software.amazon.smithy.rust.codegen.util.hasEventStreamOperations

/**
 * The NoOpEventStreamSigningDecorator:
 * - adds a `new_event_stream_signer()` method to `config` to create an Event Stream NoOp signer
 */
open class NoOpEventStreamSigningDecorator<C : CoreCodegenContext> : RustCodegenDecorator<C> {
    override val name: String = "NoOpEventStreamSigning"
    override val order: Byte = Byte.MIN_VALUE

    private fun applies(codegenContext: CoreCodegenContext, baseCustomizations: List<ConfigCustomization>): Boolean =
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
}

class NoOpEventStreamSigningConfig(
    private val serviceHasEventStream: Boolean,
    runtimeConfig: RuntimeConfig,
) : EventStreamSigningConfig(runtimeConfig) {
    private val smithyEventStream = CargoDependency.SmithyEventStream(runtimeConfig)
    private val codegenScope = arrayOf(
        "NoOpSigner" to RuntimeType("NoOpSigner", smithyEventStream, "aws_smithy_eventstream::frame"),
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
