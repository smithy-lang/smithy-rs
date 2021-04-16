/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

fun pubUseTypes(runtimeConfig: RuntimeConfig) = listOf(
    RuntimeType.Blob(runtimeConfig),
    CargoDependency.SmithyHttp(runtimeConfig).asType().member("result::SdkError"),
    RuntimeType("Config", namespace = "config", dependency = null)
)

class SmithyTypesPubUseGenerator(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection) = writable {
        when (section) {
            LibRsSection.Body -> pubUseTypes(runtimeConfig).forEach {
                rust("pub use #T;", it)
            }
        }
    }
}
