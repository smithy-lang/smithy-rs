/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [CargoDependency]; see its documentation for details.
 * For a dependency that is used in the client, or in both the client and the server, use [CargoDependency] directly.
 */
object ServerCargoDependency {
    val AsyncTrait: CargoDependency = CargoDependency("async-trait", CratesIo("0.1"))
    val AxumCore: CargoDependency = CargoDependency("axum-core", CratesIo("0.1"))
    val FuturesUtil: CargoDependency = CargoDependency("futures-util", CratesIo("0.3"))
    val PinProjectLite: CargoDependency = CargoDependency("pin-project-lite", CratesIo("0.2"))
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
}
