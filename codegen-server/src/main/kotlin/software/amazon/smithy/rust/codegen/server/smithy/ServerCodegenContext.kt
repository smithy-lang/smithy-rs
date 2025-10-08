/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.returnSymbolToParseFn

/**
 * Represents the set of HTTP-related dependencies used for code generation.
 *
 * This data class ensures that all HTTP dependencies (http, http-body, etc.) are selected
 * together as a cohesive set, preventing accidental mixing of HTTP 0.x and 1.x versions.
 */
data class HttpDependencies(
    val http: CargoDependency,
    val httpBody: CargoDependency,
    val httpBodyUtil: CargoDependency?,
    val smithyHttpServer: CargoDependency,
) {
    /**
     * Returns the http crate as a RuntimeType.
     */
    fun httpType(): RuntimeType = http.toType()

    /**
     * Returns the appropriate HTTP Request type.
     *
     * Always returns `http::Request` - the version is determined by which `http` crate
     * dependency is in Cargo.toml (controlled by the http-1x feature).
     */
    fun httpRequest(): RuntimeType = http.toType().resolve("Request")

    /**
     * Returns the appropriate HTTP Response type.
     *
     * Always returns `http::Response` - the version is determined by which `http` crate
     * dependency is in Cargo.toml.
     */
    fun httpResponse(): RuntimeType = http.toType().resolve("Response")

    /**
     * Returns the appropriate HTTP Method type.
     *
     * Always returns `http::Method` - the version is determined by which `http` crate
     * dependency is in Cargo.toml.
     */
    fun httpMethod(): RuntimeType = http.toType().resolve("Method")

    /**
     * Returns the appropriate HTTP HeaderMap type.
     *
     * Always returns `http::HeaderMap` - the version is determined by which `http` crate
     * dependency is in Cargo.toml.
     */
    fun httpHeaderMap(): RuntimeType = http.toType().resolve("HeaderMap")

    /**
     * Returns the http module as a RuntimeType for general http::* usage.
     *
     * Always returns the `http` crate - the version is determined by Cargo.toml dependencies.
     * For HTTP 0.x: references `http` crate version 0.2.x
     * For HTTP 1.x: references `http` crate version 1.x
     */
    fun httpModule(): RuntimeType = http.toType()
}

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

    /**
     * Returns the appropriate HTTP dependencies based on the http-1x configuration.
     *
     * This is the single source of truth for HTTP dependency selection. When http-1x
     * is enabled, all HTTP dependencies are upgraded together to maintain compatibility.
     */
    fun httpDependencies(): HttpDependencies =
        if (settings.codegenConfig.http1x) {
            HttpDependencies(
                http = CargoDependency.Http1x,
                httpBody = CargoDependency.HttpBody1x,
                httpBodyUtil = CargoDependency.HttpBodyUtil01x,
                smithyHttpServer = ServerCargoDependency.smithyHttpServerHttp1x(runtimeConfig),
            )
        } else {
            HttpDependencies(
                http = CargoDependency.Http,
                httpBody = CargoDependency.HttpBody,
                httpBodyUtil = null,
                smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig),
            )
        }
}
