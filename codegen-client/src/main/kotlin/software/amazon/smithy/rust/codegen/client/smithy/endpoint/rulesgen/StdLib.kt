/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.endpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * StandardLibrary functions for native-smithy implementations
 */
class NativeSmithyEndpointsStdLib : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "NativeSmithyStandardLib"
    override val order: Byte = 0

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean {
        return clazz.isAssignableFrom(ClientCodegenContext::class.java)
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(StdLib())
    }
}

internal val NativeSmithyFunctions = listOf(
    PureRuntimeFunction("substring", endpointsLib("substring").asType().member("substring")),
    PureRuntimeFunction("isValidHostLabel", endpointsLib("host").asType().member("is_valid_host_label")),
    PureRuntimeFunction(
        "parseURL",
        endpointsLib("parse_url", CargoDependency.Http, CargoDependency.Url).asType().member("parse_url"),
    ),
    PureRuntimeFunction(
        "uriEncode",
        endpointsLib("uri_encode", CargoDependency.PercentEncoding).asType().member("uri_encode"),
    ),
)

/**
 * AWS Standard library functions
 *
 * This is defined in client-codegen to support running tests—it is not used when generating smithy-native services.
 */
fun awsStandardLib(runtimeConfig: RuntimeConfig, partitionsDotJson: Node) = listOf(
    PureRuntimeFunction("aws.parseArn", endpointsLib("arn").asType().member("parse_arn")),
    PureRuntimeFunction(
        "aws.isVirtualHostableS3Bucket",
        endpointsLib(
            "s3",
            endpointsLib("host"),
            CargoDependency.OnceCell,
            CargoDependency.Regex,
        ).asType().member("is_virtual_hostable_s3_bucket"),
    ),
    AwsPartition(
        runtimeConfig,
        partitionsDotJson,
    ),
)

class AwsPartition(runtimeConfig: RuntimeConfig, private val partitionsDotJson: Node) : CustomRuntimeFunction() {
    override val id: String = "aws.partition"
    private val codegenScope = arrayOf(
        "PartitionResolver" to endpointsLib(
            "partition",
            CargoDependency.smithyJson(runtimeConfig),
            CargoDependency.Regex,
        ).asType()
            .member("PartitionResolver"),
    )

    override fun structFieldInit() = writable {
        rustTemplate(
            """partition_resolver: #{PartitionResolver}::new_from_json(b${
            Node.printJson(partitionsDotJson).dq()
            }).expect("valid JSON")""",
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

class StdLib : EndpointCustomization {
    override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> {
        return NativeSmithyFunctions
    }

    override fun builtInDefaultValue(parameter: Parameter, configRef: String): Writable? = null
}

/**
 * A runtime function that doesn't need any support structures and can be invoked directly
 */
class PureRuntimeFunction(override val id: String, private val runtimeType: RuntimeType) :
    CustomRuntimeFunction() {
    override fun structFieldInit(): Writable? = null

    override fun additionalArgsSignature(): Writable? = null

    override fun additionalArgsInvocation(self: String): Writable? = null

    override fun structField(): Writable? = null

    override fun usage() = writable { rust("#T", runtimeType) }
}
