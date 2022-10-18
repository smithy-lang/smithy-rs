/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsProtocolTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream

private fun RuntimeConfig.httpVersionModule(): RuntimeType =
    RuntimeType("http_versions", this.runtimeCrate("http"), "aws_smithy_http")
private fun RuntimeConfig.defaultHttpVersionList(): RuntimeType =
    this.httpVersionModule().member("DEFAULT_HTTP_VERSION_LIST")

class HttpVersionListCustomization(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    private val defaultHttpVersions = codegenContext.runtimeConfig.defaultHttpVersionList().fullyQualifiedName()

    override fun section(section: OperationSection): Writable {
        val awsProtocolTrait = codegenContext.serviceShape.getTrait<AwsProtocolTrait>()
        val supportedHttpProtocolVersions = if (awsProtocolTrait == null) {
            // No protocol trait was defined, use default http versions
            "$defaultHttpVersions.clone()"
        } else {
            // Figure out whether we're dealing with an EventStream operation and fetch the corresponding list of desired HTTP versions
            val versionList = if (operationShape.isEventStream(codegenContext.model)) awsProtocolTrait.eventStreamHttp else awsProtocolTrait.http
            if (versionList.isEmpty()) {
                // If no desired versions are specified, go with the default
                "$defaultHttpVersions.clone()"
            } else {
                // otherwise, use the specified versions
                "vec![${versionList.joinToString(",") { version -> mapHttpVersion(version) }}]"
            }
        }

        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                    ${section.request}.properties_mut().insert($supportedHttpProtocolVersions);
                    """,
                )
            }
            else -> emptySection
        }
    }
}

// Map an ALPN protocol ID to a version from the `http` Rust crate
private fun mapHttpVersion(httpVersion: String): String {
    return when (httpVersion) {
        "http/1.1" -> "http::Version::HTTP_11"
        "h2" -> "http::Version::HTTP_2"
        else -> TODO("Unsupported HTTP version '$httpVersion', please check your model")
    }
}
