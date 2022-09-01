/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson

interface ServerProtocol {
    fun coreProtocol(): Protocol

    fun markerStruct(): RuntimeType

    fun routerType(): RuntimeType

    companion object {
        fun fromCoreProtocol(runtimeConfig: RuntimeConfig, protocol: Protocol): ServerProtocol {
            val serverProtocol = when (protocol) {
                is AwsJson -> AwsJsonServerProtocol(runtimeConfig, protocol)
                is RestJson -> RestJsonServerProtocol(runtimeConfig, protocol)
                else -> throw IllegalStateException("unsupported protocol")
            }
            return serverProtocol
        }
    }
}

class AwsJsonServerProtocol(
    private val runtimeConfig: RuntimeConfig,
    private val coreProtocol: AwsJson,
) : ServerProtocol {
    override fun coreProtocol(): Protocol {
        return coreProtocol
    }

    override fun markerStruct(): RuntimeType {
        val name = when (coreProtocol.version) {
            is AwsJsonVersion.Json10 -> {
                "AwsJson10"
            }
            is AwsJsonVersion.Json11 -> {
                "AwsJson11"
            }
            else -> throw IllegalStateException()
        }
        return RuntimeType(name, ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
    }

    override fun routerType(): RuntimeType {
        return RuntimeType("AwsJsonRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::aws_json")
    }
}

class RestJsonServerProtocol(
    private val runtimeConfig: RuntimeConfig,
    private val coreProtocol: RestJson,
) : ServerProtocol {
    override fun coreProtocol(): Protocol {
        return coreProtocol
    }

    override fun markerStruct(): RuntimeType {
        return RuntimeType("AwsRestJson1", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")
    }

    override fun routerType(): RuntimeType {
        return RuntimeType("RestRouter", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing::routers::rest")
    }
}
