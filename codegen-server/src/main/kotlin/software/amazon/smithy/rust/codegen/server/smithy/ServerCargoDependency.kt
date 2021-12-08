/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig

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

class ServerInlineDependency(
    name: String,
    module: RustModule,
    extraDependencies: List<RustDependency> = listOf(),
    renderer: (RustWriter) -> Unit
) : InlineDependency(name, module, extraDependencies, renderer) {
    companion object {
        fun serverOperationHandler(runtimeConfig: RuntimeConfig): InlineDependency =
            forRustFile(
                "server_operation_handler_trait",
                CargoDependency.SmithyHttpServer(runtimeConfig),
                CargoDependency.Http,
                ServerCargoDependency.PinProjectLite,
                ServerCargoDependency.Tower,
                ServerCargoDependency.FuturesUtil,
                ServerCargoDependency.AsyncTrait,
            )
    }
}
