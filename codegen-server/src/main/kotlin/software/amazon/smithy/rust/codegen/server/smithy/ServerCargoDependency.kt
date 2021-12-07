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
    val Axum: CargoDependency = CargoDependency("axum", CratesIo("0.3"))
    val FuturesUtil: CargoDependency = CargoDependency("futures-util", CratesIo("0.3"))
    val PinProject: CargoDependency = CargoDependency("pin-project", CratesIo("1"))
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
    val Nom: CargoDependency = CargoDependency("nom", CratesIo("7"))
}
