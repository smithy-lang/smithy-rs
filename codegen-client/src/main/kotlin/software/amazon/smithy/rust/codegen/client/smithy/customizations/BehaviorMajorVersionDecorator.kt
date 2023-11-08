/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

class BehaviorMajorVersionDecorator(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val moduleUseName = codegenContext.moduleUseName()

    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val bmv = RuntimeType.smithyRuntimeApi(rc).resolve("client::behavior_version::BehaviorMajorVersion")
        arrayOf(
            *preludeScope,
            "BehaviorMajorVersion" to bmv,
        )
    }

    override fun section(section: ServiceConfig): Writable = writable {
    }
}
