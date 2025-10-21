/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [RuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object ServerRuntimeType {
    fun router(httpDependencies: HttpDependencies) =
        httpDependencies.smithyHttpServer.toType().resolve("routing::Router")

    fun protocol(
        name: String,
        path: String,
        httpDependencies: HttpDependencies,
    ) = httpDependencies.smithyHttpServer.toType().resolve("protocol::$path::$name")

    fun protocol(httpDependencies: HttpDependencies) = protocol("Protocol", "", httpDependencies)
}
