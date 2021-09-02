/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection

val ClippyAllowLints = listOf(
    // Sometimes operations are named the same as our module eg. output leading to `output::output`
    "module_inception",

    // Currently, we don't recase acronyms in models, eg. SSEVersion
    "upper_case_acronyms",

    // Large errors trigger this warning, we are unlikely to optimize this case currently
    "large_enum_variant",

    // Some models have members with `is` in the name, which leads to builder functions with the wrong self convention
    "wrong_self_convention",

    // models like ecs use method names like "add()" which confuses clippy
    "should_implement_trait",

    // protocol tests use silly names like `baz`, don't flag that
    "blacklisted_name",

    // Forcing use of `vec![]` can make codegen harder in some cases
    "vec_init_then_push",
)

class AllowClippyLints : LibRsCustomization() {
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
