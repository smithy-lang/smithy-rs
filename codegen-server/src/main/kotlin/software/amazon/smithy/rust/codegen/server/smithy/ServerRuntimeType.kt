/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [RuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object ServerRuntimeType {
    /** Internal body helpers used by generated code, selected for the runtime's HTTP version. */
    fun codegenBody(runtimeConfig: RuntimeConfig) =
        ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve(
            when (runtimeConfig.httpVersion) {
                HttpVersion.Http0x -> "body"
                HttpVersion.Http1x -> "body::__private"
            },
        )

    fun router(runtimeConfig: RuntimeConfig) =
        ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("routing::Router")

    fun protocol(
        name: String,
        path: String,
        runtimeConfig: RuntimeConfig,
    ) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("protocol::$path::$name")

    fun protocol(runtimeConfig: RuntimeConfig) = protocol("Protocol", "", runtimeConfig)

    /**
     * The runtime type for a built-in protocol. One type serves both as the legacy protocol type
     * parameter and as the schema-driven protocol implementation.
     */
    fun protocolType(
        protocol: ShapeId,
        runtimeConfig: RuntimeConfig,
    ): RuntimeType =
        when (protocol) {
            AwsJson1_0Trait.ID -> protocol("AwsJson1_0", "aws_json_10", runtimeConfig)
            AwsJson1_1Trait.ID -> protocol("AwsJson1_1", "aws_json_11", runtimeConfig)
            RestJson1Trait.ID -> protocol("RestJson1", "rest_json_1", runtimeConfig)
            RestXmlTrait.ID -> protocol("RestXml", "rest_xml", runtimeConfig)
            Rpcv2CborTrait.ID -> protocol("RpcV2Cbor", "rpc_v2_cbor", runtimeConfig)
            else -> throw IllegalArgumentException("No built-in server protocol type for `$protocol`")
        }
}
