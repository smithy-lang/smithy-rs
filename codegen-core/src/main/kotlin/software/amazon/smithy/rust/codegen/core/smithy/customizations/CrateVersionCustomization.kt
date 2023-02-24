/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

/**
 * Add `PGK_VERSION` const in lib.rs to enable knowing the version of the current module
 */
object CrateVersionCustomization {
    fun pkgVersion(module: RustModule): RuntimeType = RuntimeType(module.fullyQualifiedPath() + "::PKG_VERSION")

    fun extras(rustCrate: RustCrate, module: RustModule) =
        rustCrate.withModule(module) {
            rust(
                """
                /// Crate version number.
                pub static PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
                """,
            )
        }
}
