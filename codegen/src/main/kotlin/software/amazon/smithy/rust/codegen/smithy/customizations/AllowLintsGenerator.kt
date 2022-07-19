/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection

val ClippyAllowLints = listOf(
    // Sometimes operations are named the same as our module e.g. output leading to `output::output`.
    "module_inception",

    // Currently, we don't recase acronyms in models, e.g. `SSEVersion`.
    "upper_case_acronyms",

    // Large errors trigger this warning, we are unlikely to optimize this case currently.
    "large_enum_variant",

    // Some models have members with `is` in the name, which leads to builder functions with the wrong self convention.
    "wrong_self_convention",

    // Models like ecs use method names like `add()` which confuses Clippy.
    "should_implement_trait",

    // Protocol tests use silly names like `baz`, don't flag that.
    "blacklisted_name",

    // Forcing use of `vec![]` can make codegen harder in some cases.
    "vec_init_then_push",

    // Some models have shapes that generate complex Rust types (e.g. nested collection and map shapes).
    "type_complexity",
)

val AllowDocsLints = listOf(
    // Rust >=1.53.0 requires links to be wrapped in `<link>`. This is extremely hard to enforce for
    // docs that come from the modeled documentation, so we need to disable this lint
    "bare_urls"
)

class AllowLintsGenerator(
    private val bareLints: List<String> = listOf(),
    private val clippyLints: List<String> = ClippyAllowLints,
    private val docsLints: List<String> = AllowDocsLints
) : LibRsCustomization() {
    override fun section(section: LibRsSection) = when (section) {
        is LibRsSection.Attributes -> writable {
            bareLints.forEach {
                Attribute.Custom("allow($it)", container = true).render(this)
            }
            clippyLints.forEach {
                Attribute.Custom("allow(clippy::$it)", container = true).render(this)
            }
            docsLints.forEach {
                Attribute.Custom("allow(rustdoc::$it)", container = true).render(this)
            }
            // add a newline at the end
            this.write("")
        }
        else -> emptySection
    }
}
