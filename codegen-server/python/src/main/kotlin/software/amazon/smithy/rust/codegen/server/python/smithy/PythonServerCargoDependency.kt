/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig

/**
 * Object used *exclusively* in the runtime of the Python server, for separation concerns.
 * Analogous to the companion object in [CargoDependency] and [ServerCargoDependency]; see its documentation for details.
 * For a dependency that is used in the client, or in both the client and the server, use [CargoDependency] directly.
 */
object PythonServerCargoDependency {
    val PyO3: CargoDependency = CargoDependency("pyo3", CratesIo("0.16"), features = setOf("extension-module"))
    val PyO3Asyncio: CargoDependency = CargoDependency("pyo3-asyncio", CratesIo("0.16"), features = setOf("attributes", "tokio-runtime"))
    val Tokio: CargoDependency = CargoDependency("tokio", CratesIo("1.0"), features = setOf("full"))
    val Tracing: CargoDependency = CargoDependency("tracing", CratesIo("0.1"))
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
    val TowerHttp: CargoDependency = CargoDependency("tower-http", CratesIo("0.3"), features = setOf("trace"))
    val Hyper: CargoDependency = CargoDependency("hyper", CratesIo("0.14"), features = setOf("server", "http1", "http2", "tcp", "stream"))
    val NumCpus: CargoDependency = CargoDependency("num_cpus", CratesIo("1.13"))

    fun SmithyHttpServer(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http-server")
    fun SmithyHttpServerPython(runtimeConfig: RuntimeConfig) = runtimeConfig.runtimeCrate("http-server-python")
}
