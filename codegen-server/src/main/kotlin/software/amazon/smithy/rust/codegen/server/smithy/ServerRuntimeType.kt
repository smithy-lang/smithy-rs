/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [RuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object ServerRuntimeType {
    fun forInlineDependency(inlineDependency: InlineDependency) = RuntimeType("crate::${inlineDependency.name}", inlineDependency)

    fun router(runtimeConfig: RuntimeConfig) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("routing::Router")

    fun operationHandler(runtimeConfig: RuntimeConfig) =
        forInlineDependency(ServerInlineDependency.serverOperationHandler(runtimeConfig))

    fun runtimeError(runtimeConfig: RuntimeConfig) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("runtime_error::RuntimeError")

    fun requestRejection(runtimeConfig: RuntimeConfig) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("rejection::RequestRejection")

    fun responseRejection(runtimeConfig: RuntimeConfig) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("rejection::ResponseRejection")

    fun protocol(name: String, path: String, runtimeConfig: RuntimeConfig) = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("proto::$path::$name")

    fun protocol(runtimeConfig: RuntimeConfig) = protocol("Protocol", "", runtimeConfig)
}
