/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsProtocolTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.isEventStream

private fun RuntimeConfig.httpVersionModule(): RuntimeType =
    RuntimeType("http_versions", this.runtimeCrate("http"), "aws_smithy_http")
private fun RuntimeConfig.defaultHttpVersionList(): RuntimeType =
    this.httpVersionModule().member("DEFAULT_HTTP_VERSION_LIST")

class HttpVersionListCustomization(
    private val coreCodegenContext: CoreCodegenContext,
    private val operationShape: OperationShape
) : OperationCustomization() {
    private val defaultHttpVersions = coreCodegenContext.runtimeConfig.defaultHttpVersionList().fullyQualifiedName()

    override fun section(section: OperationSection): Writable {
        val awsProtocolTrait = coreCodegenContext.serviceShape.getTrait<AwsProtocolTrait>()
        val supportedHttpProtocolVersions = if (awsProtocolTrait == null) {
            // No protocol trait was defined, use default http versions
            "$defaultHttpVersions.clone()"
        } else {
            // Figure out whether we're dealing with an EventStream operation and fetch the corresponding list of desired HTTP versions
            val versionList = if (operationShape.isEventStream(coreCodegenContext.model)) awsProtocolTrait.eventStreamHttp else awsProtocolTrait.http
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
                    """
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
