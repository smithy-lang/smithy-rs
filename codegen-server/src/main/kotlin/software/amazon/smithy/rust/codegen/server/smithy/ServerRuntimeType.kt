/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.client.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType

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

    fun ConstrainedTrait(runtimeConfig: RuntimeConfig) =
        RuntimeType("Constrained", ServerCargoDependency.SmithyHttpServer(runtimeConfig), namespace = "${runtimeConfig.crateSrcPrefix}_http_server::constrained")

    fun MaybeConstrained(runtimeConfig: RuntimeConfig) =
        RuntimeType("MaybeConstrained", ServerCargoDependency.SmithyHttpServer(runtimeConfig), namespace = "${runtimeConfig.crateSrcPrefix}_http_server::constrained")

    fun Router(runtimeConfig: RuntimeConfig) =
        RuntimeType("Router", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing")

    fun RequestSpecModule(runtimeConfig: RuntimeConfig) =
        RuntimeType("request_spec", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::routing")

    fun OperationHandler(runtimeConfig: RuntimeConfig) =
        forInlineDependency(ServerInlineDependency.serverOperationHandler(runtimeConfig))

    fun RuntimeError(runtimeConfig: RuntimeConfig) =
        RuntimeType("RuntimeError", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::runtime_error")

    fun RejectionModule(runtimeConfig: RuntimeConfig) =
        RuntimeType("rejection", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server")

    // TODO Reuse above
    fun RequestRejection(runtimeConfig: RuntimeConfig) =
        RuntimeType("RequestRejection", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::rejection")

    fun ResponseRejection(runtimeConfig: RuntimeConfig) =
        RuntimeType("ResponseRejection", ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::rejection")

    fun Protocol(name: String, runtimeConfig: RuntimeConfig) =
        RuntimeType(name, ServerCargoDependency.SmithyHttpServer(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server::protocols")

    fun Protocol(runtimeConfig: RuntimeConfig) = Protocol("Protocol", runtimeConfig)
}
