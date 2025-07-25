/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig

/**
 * Object used *exclusively* in the runtime of the Typescript server, for separation concerns.
 * Analogous to the companion object in [CargoDependency] and [software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency]; see its documentation for details.
 * For a dependency that is used in the client, or in both the client and the server, use [CargoDependency] directly.
 */
object TsServerCargoDependency {
    val Napi: CargoDependency = CargoDependency("napi", CratesIo("2.11"), features = setOf("tokio_rt", "napi8"))
    val NapiDerive: CargoDependency = CargoDependency("napi-derive", CratesIo("2.11"))
    val NapiBuild: CargoDependency = CargoDependency("napi-build", CratesIo("2.0"), DependencyScope.Build)
    val Tokio: CargoDependency = CargoDependency("tokio", CratesIo("1.20.1"), features = setOf("full"))
    val Tracing: CargoDependency = CargoDependency("tracing", CratesIo("0.1"))
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
    val TowerHttp: CargoDependency = CargoDependency("tower-http", CratesIo("0.3"), features = setOf("trace"))
    val Hyper: CargoDependency =
        CargoDependency("hyper", CratesIo("0.14.12"), features = setOf("server", "http1", "http2", "tcp", "stream"))
    val NumCpus: CargoDependency = CargoDependency("num_cpus", CratesIo("1.13"))
    val ParkingLot: CargoDependency = CargoDependency("parking_lot", CratesIo("0.12"))
    val Socket2: CargoDependency = CargoDependency("socket2", CratesIo("0.4"))

    fun smithyHttpServer(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-http-server")

    fun smithyHttpServerTs(runtimeConfig: RuntimeConfig) =
        runtimeConfig.smithyRuntimeCrate("smithy-http-server-typescript")
}
