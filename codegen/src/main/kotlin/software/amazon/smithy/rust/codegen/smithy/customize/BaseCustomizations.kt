/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.rust.codegen.smithy.customizations.AllowClippyLints
import software.amazon.smithy.rust.codegen.smithy.customizations.CrateVersionGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.SmithyTypesPubUseGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

/** A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 */
class BaseCustomizations : RustCodegenDecorator {
    override val name: String = "Base"
    override val order: Byte = -1

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + CrateVersionGenerator() + SmithyTypesPubUseGenerator(protocolConfig.runtimeConfig) + AllowClippyLints()
    }
}
