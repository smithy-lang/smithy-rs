/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [CargoDependency]; see its documentation for details.
 * For a dependency that is used in the client, or in both the client and the server, use [CargoDependency] directly.
 */
object ServerCargoDependency {
    val AsyncTrait: CargoDependency = CargoDependency("async-trait", CratesIo("0.1"))
    val FormUrlEncoded: CargoDependency = CargoDependency("form_urlencoded", CratesIo("1"))
    val FuturesUtil: CargoDependency = CargoDependency("futures-util", CratesIo("0.3"))
    val Mime: CargoDependency = CargoDependency("mime", CratesIo("0.3"))
    val Nom: CargoDependency = CargoDependency("nom", CratesIo("7"))
    val OnceCell: CargoDependency = CargoDependency("once_cell", CratesIo("1.13"))
    val PinProjectLite: CargoDependency = CargoDependency("pin-project-lite", CratesIo("0.2"))
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
    val TokioDev: CargoDependency = CargoDependency("tokio", CratesIo("1.8.4"), scope = DependencyScope.Dev)
    val Regex: CargoDependency = CargoDependency("regex", CratesIo("1.5.5"))

    fun smithyHttpServer(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-http-server")
}

/**
 * A dependency on a snippet of code
 *
 * ServerInlineDependency should not be instantiated directly, rather, it should be constructed with
 * [software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.forInlineFun]
 *
 * ServerInlineDependencies are created as private modules within the main crate. This is useful for any code that
 * doesn't need to exist in a shared crate, but must still be generated exactly once during codegen.
 *
 * CodegenVisitor de-duplicates inline dependencies by (module, name) during code generation.
 */
object ServerInlineDependency {
    fun serverOperationHandler(runtimeConfig: RuntimeConfig): InlineDependency =
        InlineDependency.forRustFile(
            RustModule.private("server_operation_handler_trait"),
            "/inlineable/src/server_operation_handler_trait.rs",
            ServerCargoDependency.smithyHttpServer(runtimeConfig),
            CargoDependency.Http,
            ServerCargoDependency.PinProjectLite,
            ServerCargoDependency.Tower,
            ServerCargoDependency.FuturesUtil,
            ServerCargoDependency.AsyncTrait,
        )
}
