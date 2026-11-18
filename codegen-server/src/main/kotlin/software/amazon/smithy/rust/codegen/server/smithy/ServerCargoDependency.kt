/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig

/**
 * Object used *exclusively* in the runtime of the server, for separation concerns.
 * Analogous to the companion object in [CargoDependency]; see its documentation for details.
 * For a dependency that is used in the client, or in both the client and the server, use [CargoDependency] directly.
 */
object ServerCargoDependency {
    val AsyncTrait: CargoDependency = CargoDependency("async-trait", CratesIo("0.1.74"))
    val Base64SimdDev: CargoDependency =
        CargoDependency("base64-simd", CratesIo("0.8"), scope = DependencyScope.Dev)
    val FormUrlEncoded: CargoDependency = CargoDependency("form_urlencoded", CratesIo("1"))
    val FuturesUtil: CargoDependency = CargoDependency("futures-util", CratesIo("0.3"))
    val Mime: CargoDependency = CargoDependency("mime", CratesIo("0.3"))
    val Nom: CargoDependency = CargoDependency("nom", CratesIo("7"))
    val PinProjectLite: CargoDependency = CargoDependency("pin-project-lite", CratesIo("0.2"))
    val ThisError: CargoDependency = CargoDependency("thiserror", CratesIo("1.0"))

    /** Preserve the Tower dependency used by existing generated servers and decorators. */
    val Tower: CargoDependency = CargoDependency("tower", CratesIo("0.4"))
    private val schemaTower: CargoDependency = CargoDependency("tower", CratesIo("0.5"), features = setOf("util"))

    /**
     * Schema routing uses Tower 0.5's Sync service utilities. Generated serde retains
     * the mainline Tower 0.4 declaration, including its transitive runtime features.
     */
    fun tower(codegenContext: CodegenContext): CargoDependency =
        if ((codegenContext as? ServerCodegenContext)?.usesSchemaHttpSerde == true) {
            schemaTower
        } else {
            Tower
        }

    val TokioDev: CargoDependency =
        CargoDependency("tokio", CratesIo("1.23.1"), scope = DependencyScope.Dev)
    val TokioTime: CargoDependency =
        CargoDependency("tokio", CratesIo("1.23.1"), features = setOf("time"))
    val Regex: CargoDependency = CargoDependency("regex", CratesIo("1.5.5"))

    /**
     * Returns the appropriate smithy-http-server dependency based on HTTP version.
     *
     * For HTTP 1.x: returns `aws-smithy-http-server` (latest version)
     * For HTTP 0.x: returns `aws-smithy-legacy-http-server` (forked version supporting http@0.2)
     */
    fun smithyHttpServer(runtimeConfig: RuntimeConfig): CargoDependency =
        when (runtimeConfig.httpVersion) {
            HttpVersion.Http1x -> runtimeConfig.smithyRuntimeCrate("smithy-http-server")
            HttpVersion.Http0x -> runtimeConfig.smithyRuntimeCrate("smithy-legacy-http-server")
        }

    fun hyperDev(runtimeConfig: RuntimeConfig): CargoDependency =
        CargoDependency.hyper(runtimeConfig).copy(scope = DependencyScope.Dev)

    fun smithyTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-types")
}
