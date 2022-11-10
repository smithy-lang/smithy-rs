/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency.smithyHttpServer

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [RuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object ServerRuntimeType {
    private fun forInlineDependency(inlineDependency: InlineDependency) = RuntimeType.forInlineDependency(inlineDependency)

    fun Router(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("routing::Router")

    fun RequestSpecModule(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("routing::request_spec")

    fun OperationHandler(runtimeConfig: RuntimeConfig) =
        forInlineDependency(ServerInlineDependency.serverOperationHandler(runtimeConfig))

    fun RuntimeError(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("runtime_error::RuntimeError")

    fun RequestRejection(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("rejection::RequestRejection")

    fun ResponseRejection(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("rejection::ResponseRejection")

    fun Protocol(runtimeConfig: RuntimeConfig) = smithyHttpServer(runtimeConfig).toType().resolve("proto")
}
