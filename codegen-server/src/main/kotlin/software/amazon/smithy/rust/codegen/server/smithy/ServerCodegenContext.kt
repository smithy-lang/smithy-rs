/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.returnSymbolToParseFn

/**
 * [ServerCodegenContext] contains code-generation context that is _specific_ to the [RustServerCodegenPlugin] plugin
 * from the `rust-codegen-server` subproject.
 *
 * It inherits from [CodegenContext], which contains code-generation context that is common to _all_ smithy-rs plugins.
 *
 * This class has to live in the `codegen` subproject because it is referenced in common generators to both client
 * and server (like [JsonParserGenerator]).
 */
data class ServerCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val moduleDocProvider: ModuleDocProvider?,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ServerRustSettings,
    val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    val constrainedShapeSymbolProvider: RustSymbolProvider,
    val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
) : CodegenContext(
        model, symbolProvider, moduleDocProvider, serviceShape, protocol, settings, CodegenTarget.SERVER,
    ) {
    override fun builderInstantiator(): BuilderInstantiator {
        return ServerBuilderInstantiator(symbolProvider, returnSymbolToParseFn(this))
    }

    fun isHttp1() = settings.codegenConfig.http1x

    /**
     * Returns the appropriate HTTP dependencies based on the http-1x configuration.
     *
     * This is the single source of truth for HTTP dependency selection. When http-1x
     * is enabled, all HTTP dependencies are upgraded together to maintain compatibility.
     */
    private val httpDependencies by lazy {
        HttpDependenciesFactory.create(settings.codegenConfig.http1x, runtimeConfig)
    }

    fun httpDependencies(): HttpDependencies = httpDependencies
}

/**
 * Factory for creating HttpDependencies based on http-1x configuration.
 *
 * This factory is used in multiple places:
 * - ServerCodegenContext (lazy initialization)
 * - RustServerCodegenPlugin (for EventStreamSymbolProvider)
 */
object HttpDependenciesFactory {
    fun create(http1x: Boolean, runtimeConfig: software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig): HttpDependencies {
        return if (http1x) {
            println("[HttpDependenciesFactory] Creating HTTP 1.x dependencies")
            HttpDependencies(
                http = CargoDependency.Http1x,
                httpBody = CargoDependency.HttpBody1x,
                httpBodyUtil = CargoDependency.HttpBodyUtil01x,
                hyper = CargoDependency("hyper", CratesIo("1")),
                hyperDev = CargoDependency("hyper", CratesIo("1"), scope = DependencyScope.Dev),
                smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig),
                smithyRuntimeApi = CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-1x"),
                smithyTypes = CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-1-x"),
                smithyHttp = CargoDependency.smithyHttp(runtimeConfig),
                smithyJson = CargoDependency.smithyJson(runtimeConfig),
                smithyCbor = CargoDependency.smithyCbor(runtimeConfig),
                smithyXml = CargoDependency.smithyXml(runtimeConfig),
            )
        } else {
            println("[HttpDependenciesFactory] Creating HTTP 0.x dependencies")
            HttpDependencies(
                http = CargoDependency("http-0x", CratesIo("0.2.9"), `package` = "http"),
                httpBody = CargoDependency("http-body-0x", CratesIo("0.4.4"), `package` = "http-body"),
                httpBodyUtil = null,
                hyper = CargoDependency.Hyper,
                hyperDev = ServerCargoDependency.HyperDev,
                smithyHttpServer = ServerCargoDependency.smithyHttpLegacyServer(runtimeConfig),
                smithyRuntimeApi = CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-02x"),
                smithyTypes = CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-0-4-x"),
                smithyHttp = ServerCargoDependency.smithyHttpLegacy(runtimeConfig),
                smithyJson = CargoDependency.smithyJson(runtimeConfig),
                smithyCbor = CargoDependency.smithyCbor(runtimeConfig),
                smithyXml = CargoDependency.smithyXml(runtimeConfig),
            )
        }
    }
}
