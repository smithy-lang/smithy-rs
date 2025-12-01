/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointStdLib
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Standard library functions available to all generated crates (e.g. not `aws.` specific / prefixed)
 */
internal val SmithyEndpointsStdLib: List<CustomRuntimeFunction> =
    listOf(
        SimpleRuntimeFunction("substring", EndpointsLib.substring, RuntimeType.lifetimeStr()),
        SimpleRuntimeFunction("isValidHostLabel", EndpointsLib.isValidHostLabel, RuntimeType.Bool),
        SimpleRuntimeFunction("parseURL", EndpointsLib.parseUrl, EndpointsLib.url()),
        SimpleRuntimeFunction("uriEncode", EndpointsLib.uriEncode, RuntimeType.lifetimeCow()),
        // The runtime types for coalesce and evaluate_bdd are a bit of a lie since the return type is generic.
        // In practice they aren't actually used.
        SimpleRuntimeFunction("coalesce", EndpointsLib.coalesce, RuntimeType.Option),
        SimpleRuntimeFunction("evaluate_bdd", EndpointsLib.evaluateBdd, RuntimeType.Option),
    )

/**
 * AWS Standard library functions
 *
 * This is defined in client-codegen to support running tests—it is not used when generating smithy-native services.
 */
fun awsStandardLib(
    runtimeConfig: RuntimeConfig,
    partitionsDotJson: Node,
) = listOf(
    SimpleRuntimeFunction("aws.parseArn", EndpointsLib.awsParseArn, EndpointsLib.arn()),
    SimpleRuntimeFunction("aws.isVirtualHostableS3Bucket", EndpointsLib.awsIsVirtualHostableS3Bucket, RuntimeType.Bool),
    AwsPartitionResolver(runtimeConfig, partitionsDotJson),
)

/**
 * Implementation of the `aws.partition` standard library function.
 *
 * A default `partitionsDotJson` node MUST be provided. The node MUST contain an AWS partition.
 */
class AwsPartitionResolver(val runtimeConfig: RuntimeConfig, private val partitionsDotJson: Node) :
    CustomRuntimeFunction() {
    override val id: String = "aws.partition"
    private val codegenScope =
        arrayOf(
            "Partition" to EndpointsLib.partition(runtimeConfig),
            "PartitionResolver" to EndpointsLib.partitionResolver(runtimeConfig),
            "tracing" to RuntimeType.Tracing,
        )

    private val defaultPartitionResolver =
        RuntimeType.forInlineFun("DEFAULT_PARTITION_RESOLVER", EndpointStdLib) {
            rustTemplate(
                """
                // Loading the partition JSON is expensive since it involves many regex compilations,
                // so cache the result so that it only need to be paid for the first constructed client.
                pub(crate) static DEFAULT_PARTITION_RESOLVER: std::sync::LazyLock<#{PartitionResolver}> =
                    std::sync::LazyLock::new(|| {
                        match std::env::var("SMITHY_CLIENT_SDK_CUSTOM_PARTITION") {
                            Ok(partitions) => {
                                #{tracing}::debug!("loading custom partitions located at {partitions}");
                                let partition_dot_json = std::fs::read_to_string(partitions).expect("should be able to read a custom partition JSON");
                                #{PartitionResolver}::new_from_json(partition_dot_json.as_bytes()).expect("valid JSON")
                            },
                            _ => {
                                #{tracing}::debug!("loading default partitions");
                                #{PartitionResolver}::new_from_json(b${
                    Node.printJson(partitionsDotJson).dq()
                }).expect("valid JSON")
                            }
                        }
                    });
                """,
                *codegenScope,
            )
        }

    override fun structFieldInit() =
        writable {
            rustTemplate(
                """partition_resolver: #{DEFAULT_PARTITION_RESOLVER}.clone()""",
                *codegenScope,
                "DEFAULT_PARTITION_RESOLVER" to
                    defaultPartitionResolver,
            )
        }

    override fun structFieldInitBdd() =
        writable {
            val json = Node.printJson(partitionsDotJson).dq()
            rustTemplate(
                """partition_resolver: &#{DEFAULT_PARTITION_RESOLVER}""",
                *codegenScope,
                "DEFAULT_PARTITION_RESOLVER" to
                    defaultPartitionResolver,
            )
        }

    override fun additionalArgsSignature(): Writable =
        writable {
            rustTemplate("partition_resolver: &#{PartitionResolver}", *codegenScope)
        }

    override fun additionalArgsSignatureBdd(): Writable =
        writable {
            rustTemplate("partition_resolver: &'a #{PartitionResolver}", *codegenScope)
        }

    override fun additionalArgsInvocation(self: String) =
        writable {
            rust("&$self.partition_resolver")
        }

    override fun structField(): Writable =
        writable {
            rustTemplate("partition_resolver: #{PartitionResolver}", *codegenScope)
        }

    override fun structFieldBdd(): Writable =
        writable {
            rustTemplate("partition_resolver: &'static #{PartitionResolver}", *codegenScope)
        }

    override fun usage() = writable { rust("partition_resolver.resolve_partition") }

    override fun returnType(): RuntimeType = EndpointsLib.partition(runtimeConfig)
}

/**
 * A runtime function that doesn't need any support structures and can be invoked directly.
 *
 * Currently, this is every runtime function other than `aws.partition`.
 */
private class SimpleRuntimeFunction(
    override val id: String,
    private val runtimeType: RuntimeType,
    val returnType: RuntimeType,
) :
    CustomRuntimeFunction() {
    override fun structFieldInit(): Writable? = null

    override fun structFieldInitBdd(): Writable? = null

    override fun additionalArgsSignature(): Writable? = null

    override fun additionalArgsSignatureBdd(): Writable? = null

    override fun additionalArgsInvocation(self: String): Writable? = null

    override fun structField(): Writable? = null

    override fun structFieldBdd(): Writable? = null

    override fun usage() = writable { rust("#T", runtimeType) }

    override fun returnType() = returnType
}
