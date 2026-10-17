/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

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
}
