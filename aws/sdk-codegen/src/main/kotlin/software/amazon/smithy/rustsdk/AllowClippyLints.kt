/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

val ClippyAllowLints = listOf("module_inception", "upper_case_acronyms")

class AllowClippyLintsDecorator() : RustCodegenDecorator {
    override val name: String = "AllowClippyLints"
    override val order: Byte = 0
    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + AllowClippyLints()
    }
}

class AllowClippyLints() : LibRsCustomization() {
    override fun section(section: LibRsSection) = when (section) {
        is LibRsSection.Attributes -> writable {
            ClippyAllowLints.forEach {
                Attribute.Custom("allow(clippy::$it)", container = true).render(this)
            }
            // add a newline at the end
            this.write("")
        }
        else -> emptySection
    }
}
