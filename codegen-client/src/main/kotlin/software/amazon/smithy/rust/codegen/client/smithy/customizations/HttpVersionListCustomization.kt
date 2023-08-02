/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsProtocolTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream

// TODO(enableNewSmithyRuntimeCleanup): Delete this file

class HttpVersionListCustomization(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        val runtimeConfig = codegenContext.runtimeConfig
        val awsProtocolTrait = codegenContext.serviceShape.getTrait<AwsProtocolTrait>()
        val supportedHttpProtocolVersions = if (awsProtocolTrait == null) {
            // No protocol trait was defined, use default http versions
            "#{defaultHttpVersions}.clone()"
        } else {
            // Figure out whether we're dealing with an EventStream operation and fetch the corresponding list of desired HTTP versions
            val versionList = if (operationShape.isEventStream(codegenContext.model)) awsProtocolTrait.eventStreamHttp else awsProtocolTrait.http
            if (versionList.isEmpty()) {
                // If no desired versions are specified, go with the default
                "#{defaultHttpVersions}.clone()"
            } else {
                // otherwise, use the specified versions
                "vec![${versionList.joinToString(",") { version -> mapHttpVersion(version) }}]"
            }
        }

        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rustTemplate(
                    """
                    ${section.request}.properties_mut().insert($supportedHttpProtocolVersions);
                    """,
                    "defaultHttpVersions" to RuntimeType.smithyHttp(runtimeConfig).resolve("http_versions::DEFAULT_HTTP_VERSION_LIST"),
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
