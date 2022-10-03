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
    fun forInlineDependency(inlineDependency: InlineDependency) =
        RuntimeType(inlineDependency.name, inlineDependency, namespace = "crate")

    val Phantom = RuntimeType("PhantomData", dependency = null, namespace = "std::marker")
    val Cow = RuntimeType("Cow", dependency = null, namespace = "std::borrow")

    fun Router(runtimeConfig: RuntimeConfig) =
        RuntimeType("Router", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing")

    fun OperationHandler(runtimeConfig: RuntimeConfig) =
        forInlineDependency(ServerInlineDependency.serverOperationHandler(runtimeConfig))

    fun RuntimeError(runtimeConfig: RuntimeConfig) =
        RuntimeType("RuntimeError", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::runtime_error")

    fun RequestRejection(runtimeConfig: RuntimeConfig) =
        RuntimeType("RequestRejection", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::rejection")

    fun ResponseRejection(runtimeConfig: RuntimeConfig) =
        RuntimeType("ResponseRejection", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::rejection")

    fun Protocol(name: String, path: String, runtimeConfig: RuntimeConfig) =
        RuntimeType(name, ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::proto::" + path)

    fun Protocol(runtimeConfig: RuntimeConfig) = Protocol("Protocol", "", runtimeConfig)
}
