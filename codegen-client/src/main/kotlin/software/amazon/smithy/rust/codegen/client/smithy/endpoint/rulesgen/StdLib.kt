/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.endpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Standard library functions available to all generated crates (e.g. not `aws.` specific / prefixed)
 */
internal val SmithyEndpointsStdLib: List<CustomRuntimeFunction> = listOf(
    SimpleRuntimeFunction("substring", endpointsLib("substring").toType().resolve("substring")),
    SimpleRuntimeFunction("isValidHostLabel", endpointsLib("host").toType().resolve("is_valid_host_label")),
    SimpleRuntimeFunction(
        "parseURL",
        endpointsLib("parse_url", CargoDependency.Http, CargoDependency.Url).toType().resolve("parse_url"),
    ),
    SimpleRuntimeFunction(
        "uriEncode",
        endpointsLib("uri_encode", CargoDependency.PercentEncoding).toType().resolve("uri_encode"),
    ),
)

/**
 * AWS Standard library functions
 *
 * This is defined in client-codegen to support running tests—it is not used when generating smithy-native services.
 */
fun awsStandardLib(runtimeConfig: RuntimeConfig, partitionsDotJson: Node) = listOf(
    SimpleRuntimeFunction("aws.parseArn", endpointsLib("arn").toType().resolve("parse_arn")),
    SimpleRuntimeFunction(
        "aws.isVirtualHostableS3Bucket",
        endpointsLib(
            "s3",
            endpointsLib("host"),
            CargoDependency.OnceCell,
            CargoDependency.Regex,
        ).toType().resolve("is_virtual_hostable_s3_bucket"),
    ),
    AwsPartitionResolver(
        runtimeConfig,
        partitionsDotJson,
    ),
)

/**
 * Implementation of the `aws.partition` standard library function.
 *
 * A default `partitionsDotJson` node MUST be provided. The node MUST contain an AWS partition.
 */
class AwsPartitionResolver(runtimeConfig: RuntimeConfig, private val partitionsDotJson: Node) :
    CustomRuntimeFunction() {
    override val id: String = "aws.partition"
    private val codegenScope = arrayOf(
        "PartitionResolver" to endpointsLib(
            "partition",
            CargoDependency.smithyJson(runtimeConfig),
            CargoDependency.Regex,
        ).toType()
            .resolve("PartitionResolver"),
    )

    override fun structFieldInit() = writable {
        val json = Node.printJson(partitionsDotJson).dq()
        rustTemplate(
            """partition_resolver: #{PartitionResolver}::new_from_json(b$json).expect("valid JSON")""",
            *codegenScope,
        )
    }

    override fun additionalArgsSignature(): Writable = writable {
        rustTemplate("partition_resolver: &#{PartitionResolver}", *codegenScope)
    }

    override fun additionalArgsInvocation(self: String) = writable {
        rust("&$self.partition_resolver")
    }

    override fun structField(): Writable = writable {
        rustTemplate("partition_resolver: #{PartitionResolver}", *codegenScope)
    }

    override fun usage() = writable {
        rust("partition_resolver.resolve_partition")
    }
}

/**
 * A runtime function that doesn't need any support structures and can be invoked directly.
 *
 * Currently, this is every runtime function other than `aws.partition`.
 */
private class SimpleRuntimeFunction(override val id: String, private val runtimeType: RuntimeType) :
    CustomRuntimeFunction() {
    override fun structFieldInit(): Writable? = null

    override fun additionalArgsSignature(): Writable? = null

    override fun additionalArgsInvocation(self: String): Writable? = null

    override fun structField(): Writable? = null

    override fun usage() = writable { rust("#T", runtimeType) }
}
