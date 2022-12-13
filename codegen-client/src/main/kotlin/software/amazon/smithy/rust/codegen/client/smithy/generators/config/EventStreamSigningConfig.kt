/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

open class EventStreamSigningConfig(
    runtimeConfig: RuntimeConfig,
) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "SharedPropertyBag" to RuntimeType.smithyHttp(runtimeConfig).resolve("property_bag::SharedPropertyBag"),
        "SignMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::SignMessage"),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> configImplSection()
            else -> emptySection
        }
    }

    open fun configImplSection(): Writable = emptySection

    fun renderEventStreamSignerFn(signerInstantiator: (String) -> Writable): Writable = writable {
        rustTemplate(
            """
            /// Creates a new Event Stream `SignMessage` implementor.
            pub fn new_event_stream_signer(
                &self,
                _properties: #{SharedPropertyBag}
            ) -> impl #{SignMessage} {
                #{signer:W}
            }
            """,
            *codegenScope,
            "signer" to signerInstantiator("_properties"),
        )
    }
}
