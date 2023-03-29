/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class ConfigBagBackedConfiguration(private val runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.BuilderStruct -> {
                rust("config_bag: #T", RuntimeType.configBag(runtimeConfig))
            }

            ServiceConfig.BuilderBuild -> {
                rust("config_bag: self.config_bag.freeze()")
            }

            ServiceConfig.ConfigStruct ->
                rust(
                    "config_bag: #T",
                    RuntimeType.frozenConfigBag(runtimeConfig),
                )

            else -> {}
        }
    }
}
