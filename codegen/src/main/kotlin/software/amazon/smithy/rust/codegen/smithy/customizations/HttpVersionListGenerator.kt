/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection

private fun RuntimeConfig.httpVersionModule(): RuntimeType = RuntimeType("http_versions", this.runtimeCrate("http"), "aws_smithy_http")
private fun RuntimeConfig.defaultHttpVersionList(): RuntimeType = this.httpVersionModule().member("DEFAULT_HTTP_VERSION_LIST")

class HttpVersionListGenerator(
    private val codegenContext: CodegenContext,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                val runtimeConfig = codegenContext.runtimeConfig
                rust(
                    """
                    ${section.request}.properties_mut().insert(${runtimeConfig.defaultHttpVersionList().fullyQualifiedName()}.clone());
                    """
                )
            }
            else -> emptySection
        }
    }
}
