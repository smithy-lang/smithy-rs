/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.allow
import software.amazon.smithy.rust.codegen.core.rustlang.AttributeKind
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection

private val allowedRustcLints =
    listOf(
        // Deprecated items should be safe to compile, so don't block the compilation.
        "deprecated",
        // Unknown lints need to be allowed since we use both nightly and our MSRV, and sometimes we need
        // to disable lints that are in nightly but don't exist in the MSRV.
        "unknown_lints",
    )

private val allowedClippyLints =
    listOf(
        // Sometimes operations are named the same as our module e.g. output leading to `output::output`.
        "module_inception",
        // Currently, we don't re-case acronyms in models, e.g. `SSEVersion`.
        "upper_case_acronyms",
        // Large errors trigger this warning, we are unlikely to optimize this case currently.
        "large_enum_variant",
        // Some models have members with `is` in the name, which leads to builder functions with the wrong self convention.
        "wrong_self_convention",
        // Models like ecs use method names like `add()` which confuses Clippy.
        "should_implement_trait",
        // Protocol tests use silly names like `baz`, don't flag that.
        "disallowed_names",
        // Forcing use of `vec![]` can make codegen harder in some cases.
        "vec_init_then_push",
        // Some models have shapes that generate complex Rust types (e.g. nested collection and map shapes).
        "type_complexity",
        // Determining if the expression is the last one (to remove return) can make codegen harder in some cases.
        "needless_return",
        // For backwards compatibility, we often don't derive Eq
        "derive_partial_eq_without_eq",
        // Keeping errors small in a backwards compatible way is challenging
        "result_large_err",
        // Difficult to avoid in generated code
        "unnecessary_map_on_constructor",
        // Service models can specify a date, such as 2024-01-08, as the "since" date for deprecation.
        "deprecated_semver",
    )

private val allowedRustdocLints =
    listOf(
        // Rust >=1.53.0 requires links to be wrapped in `<link>`. This is extremely hard to enforce for
        // docs that come from the modeled documentation, so we need to disable this lint
        "bare_urls",
        // Rustdoc warns about redundant explicit links in doc comments. This is fine for handwritten
        // crates, but is impractical to manage for code generated crates. Thus, allow it.
        "redundant_explicit_links",
        // The documentation directly from the model may contain invalid HTML tags. For instance,
        // <p><code><bucketloggingstatus xmlns="http://doc.s3.amazonaws.com/2006-03-01" /></code></p>
        // is considered an invalid self-closing HTML tag `bucketloggingstatus`
        "invalid_html_tags",
    )

class AllowLintsCustomization(
    private val rustcLints: List<String> = allowedRustcLints,
    private val clippyLints: List<String> = allowedClippyLints,
    private val rustdocLints: List<String> = allowedRustdocLints,
) : LibRsCustomization() {
    override fun section(section: LibRsSection) =
        when (section) {
            is LibRsSection.Attributes ->
                writable {
                    rustcLints.forEach {
                        Attribute(allow(it)).render(this, AttributeKind.Inner)
                    }
                    clippyLints.forEach {
                        Attribute(allow("clippy::$it")).render(this, AttributeKind.Inner)
                    }
                    rustdocLints.forEach {
                        Attribute(allow("rustdoc::$it")).render(this, AttributeKind.Inner)
                    }
                    // add a newline at the end
                    this.write("")
                }
            else -> emptySection
        }
}
