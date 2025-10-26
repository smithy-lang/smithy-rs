/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

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
    val hyper: CargoDependency,
    val hyperDev: CargoDependency,
    val smithyHttpServer: CargoDependency,
    val smithyRuntimeApi: CargoDependency,
    val smithyTypes: CargoDependency,
    val smithyHttp: CargoDependency,
    val smithyJson: CargoDependency,
    val smithyCbor: CargoDependency,
    val smithyXml: CargoDependency,
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

    /**
     * Returns the http-body crate as a RuntimeType.
     *
     * For HTTP 0.x: references `http-body` crate version 0.4.x
     * For HTTP 1.x: references `http-body` crate version 1.x
     */
    fun httpBodyModule(): RuntimeType = httpBody.toType()

    /**
     * Returns the hyper crate as a RuntimeType.
     *
     * For HTTP 0.x: references `hyper` crate version 0.14.x
     * For HTTP 1.x: references `hyper` crate version 1.x
     */
    fun hyperModule(): RuntimeType = hyper.toType()

    /**
     * Returns the hyper crate as a dev dependency RuntimeType.
     *
     * For HTTP 0.x: references `hyper` crate version 0.14.x
     * For HTTP 1.x: references `hyper` crate version 1.x
     */
    fun hyperDevModule(): RuntimeType = hyperDev.toType()

    /**
     * Returns the aws-smithy-runtime-api crate as a RuntimeType.
     *
     * For HTTP 0.x: no special features
     * For HTTP 1.x: includes the "http-1x" feature
     */
    fun smithyRuntimeApiModule(): RuntimeType = smithyRuntimeApi.toType()

    /**
     * Returns the aws-smithy-types crate as a RuntimeType.
     *
     * For HTTP 0.x: no special features
     * For HTTP 1.x: includes the "http-body-1-x" feature
     */
    fun smithyTypesModule(): RuntimeType = smithyTypes.toType()

    /**
     * Returns the aws-smithy-http crate as a RuntimeType.
     *
     * For HTTP 0.x: pinned to version 0.62.x (last version supporting http@0)
     * For HTTP 1.x: uses the default version from RuntimeConfig
     */
    fun smithyHttpModule(): RuntimeType = smithyHttp.toType()

    /**
     * Returns the aws-smithy-json crate as a RuntimeType.
     *
     * For HTTP 0.x: pinned to version 0.61.x (last version supporting http@0)
     * For HTTP 1.x: uses the default version from RuntimeConfig
     */
    fun smithyJsonModule(): RuntimeType = smithyJson.toType()

    /**
     * Returns the aws-smithy-cbor crate as a RuntimeType.
     *
     * For HTTP 0.x: pinned to version 0.61.x (last version supporting http@0)
     * For HTTP 1.x: uses the default version from RuntimeConfig
     */
    fun smithyCborModule(): RuntimeType = smithyCbor.toType()

    /**
     * Returns the aws-smithy-xml crate as a RuntimeType.
     *
     * For HTTP 0.x: pinned to version 0.60.x (last version supporting http@0)
     * For HTTP 1.x: uses the default version from RuntimeConfig
     */
    fun smithyXmlModule(): RuntimeType = smithyXml.toType()

    /**
     * Returns a map of dependency names to CargoDependency objects that need pinning.
     *
     * This is used by decorators to apply manifest customizations for HTTP 0.x dependencies
     * that require specific versions or features.
     *
     * Returns a map containing only dependencies that have customizations to apply
     * (version pinning or feature flags).
     */
    fun dependenciesToPin(): Map<String, CargoDependency> =
        mapOf(
            "aws-smithy-json" to smithyJson,
            "aws-smithy-http" to smithyHttp,
        )

    companion object {
        /**
         * Factory method for creating HttpDependencies based on http-1x configuration.
         *
         * This factory is used in multiple places:
         * - ServerCodegenContext (lazy initialization)
         * - RustServerCodegenPlugin (for EventStreamSymbolProvider)
         * - Server protocol generators (for EventStream marshallers/unmarshallers)
         */
        fun create(http1x: Boolean, runtimeConfig: software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig): HttpDependencies {
            return if (http1x) {
                println("[HttpDependencies.create] Creating HTTP 1.x dependencies")
                HttpDependencies(
                    http = CargoDependency.Http1x,
                    httpBody = CargoDependency.HttpBody1x,
                    httpBodyUtil = CargoDependency.HttpBodyUtil01x,
                    hyper = CargoDependency("hyper", software.amazon.smithy.rust.codegen.core.rustlang.CratesIo("1")),
                    hyperDev = CargoDependency("hyper", software.amazon.smithy.rust.codegen.core.rustlang.CratesIo("1"), scope = software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope.Dev),
                    smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig),
                    smithyRuntimeApi = CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-1x"),
                    smithyTypes = CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-1-x"),
                    smithyHttp = CargoDependency.smithyHttp(runtimeConfig),
                    smithyJson = CargoDependency.smithyJson(runtimeConfig),
                    smithyCbor = CargoDependency.smithyCbor(runtimeConfig),
                    smithyXml = CargoDependency.smithyXml(runtimeConfig),
                )
            } else {
                println("[HttpDependencies.create] Creating HTTP 0.x dependencies")
                HttpDependencies(
                    http = CargoDependency("http-0x", software.amazon.smithy.rust.codegen.core.rustlang.CratesIo("0.2.9"), `package` = "http"),
                    httpBody = CargoDependency("http-body-0x", software.amazon.smithy.rust.codegen.core.rustlang.CratesIo("0.4.4"), `package` = "http-body"),
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
}
