/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [RuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object ServerRuntimeType {
    fun router(runtimeConfig: RuntimeConfig) =
        ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("routing::Router")

    fun protocol(name: String, path: String, runtimeConfig: RuntimeConfig) =
        ServerCargoDependency.smithyHttpServer(runtimeConfig).toType().resolve("protocol::$path::$name")

    fun protocol(runtimeConfig: RuntimeConfig) = protocol("Protocol", "", runtimeConfig)
}
