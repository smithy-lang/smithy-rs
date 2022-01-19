/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection

fun pubUseTypes(runtimeConfig: RuntimeConfig) = listOf(
    RuntimeType.Blob(runtimeConfig),
    RuntimeType.DateTime(runtimeConfig),
) + CargoDependency.SmithyHttp(runtimeConfig).asType().let { http ->
    listOf(
        http.member("result::SdkError"),
        http.member("byte_stream::ByteStream"),
        http.member("byte_stream::AggregatedBytes")
    )
}

class SmithyTypesPubUseGenerator(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection) = writable {
        when (section) {
            LibRsSection.Body -> {
                val types = pubUseTypes(runtimeConfig)
                if (types.isNotEmpty()) {
                    docs("Re-exported types from supporting crates.")
                    rustBlock("pub mod types") {
                        pubUseTypes(runtimeConfig).forEach {
                            rust("pub use #T;", it)
                        }
                    }
                }
            }
            else -> {}
        }
    }
}
