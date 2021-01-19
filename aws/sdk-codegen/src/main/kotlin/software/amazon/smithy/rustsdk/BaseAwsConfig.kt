/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/**
 * Just a Stub
 *
 * Augment the config object with the AWS-specific fields like service and region
 */
class BaseAwsConfig : ConfigCustomization() {
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.ConfigStruct -> {
                Attribute.AllowUnused.render(this)
                rust("pub(crate) region: String,")
            }
            ServiceConfig.BuilderBuild -> rust("region: \"todo\".to_owned(),")
            else -> {}
            /*ServiceConfig.ConfigImpl -> TODO()
            ServiceConfig.BuilderStruct -> TODO()
            ServiceConfig.BuilderImpl -> TODO()
            ServiceConfig.BuilderBuild -> TODO()*/
        }
    }
}
