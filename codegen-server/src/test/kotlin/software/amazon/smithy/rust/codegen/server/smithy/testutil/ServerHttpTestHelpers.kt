/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * Central location for HTTP version-agnostic test helpers.
 * These helpers work with both HTTP 0.x and HTTP 1.x depending on the codegenContext.
 */
object ServerHttpTestHelpers {
    /**
     * Returns a RuntimeType scope that includes the correct HTTP types for the given context.
     * This should be used instead of hardcoding RuntimeType.Http, RuntimeType.Hyper, etc.
     */
    fun getHttpRuntimeTypeScope(codegenContext: ServerCodegenContext): Array<Pair<String, RuntimeType>> {
        val httpModule =
            if (codegenContext.runtimeConfig.httpVersion == HttpVersion.Http0x) {
                CargoDependency.Http
            } else {
                CargoDependency.Http1x
            }
        return arrayOf(
            "Http" to httpModule.toType(),
            "Hyper" to RuntimeType.hyperForConfig(codegenContext.runtimeConfig),
            "Tower" to RuntimeType.Tower,
            *RuntimeType.preludeScope,
        )
    }

    /**
     * Creates a writable that generates code to create an HTTP body from bytes.
     *
     * For HTTP 0.x: `smithy_http_server::body::Body::from(bytes)` (legacy)
     * For HTTP 1.x: `http_body_util::Full::new(Bytes::from(bytes))`
     *
     * Note: The `bytesVariable` should be a variable containing bytes data (Vec<u8>, Bytes, etc.)
     */
    fun createBodyFromBytes(
        codegenContext: ServerCodegenContext,
        bytesVariable: String,
    ): Writable =
        writable {
            if (codegenContext.runtimeConfig.httpVersion == HttpVersion.Http1x) {
                rustTemplate(
                    """#{HttpBodyUtilFull}::new(#{Bytes}::from($bytesVariable))""",
                    "HttpBodyUtilFull" to CargoDependency.HttpBodyUtil01x.toType().resolve("Full"),
                    "Bytes" to RuntimeType.Bytes,
                )
            } else {
                rustTemplate(
                    """#{SmithyHttpServerBody}::from($bytesVariable)""",
                    "SmithyHttpServerBody" to
                        ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig)
                            .toType().resolve("body").resolve("Body"),
                )
            }
        }

    /**
     * Returns a Writable that generates version-appropriate code for reading HTTP response body to bytes.
     * For HTTP 1.x: Uses http_body_util::BodyExt::collect()
     * For HTTP 0.x: Uses hyper::body::to_bytes()
     *
     * @param responseVarName The name of the HTTP response variable (e.g., "http_response")
     */
    fun httpBodyToBytes(
        runtimeConfig: RuntimeConfig,
        bodyVarName: String,
        responseVarName: String,
    ): Writable =
        writable {
            when (runtimeConfig.httpVersion) {
                HttpVersion.Http1x ->
                    rustTemplate(
                        """
                        use #{HttpBodyUtil}::BodyExt;
                        let $bodyVarName = $responseVarName.into_body().collect().await.expect("unable to collect body").to_bytes();
                        """,
                        "HttpBodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                    )

                HttpVersion.Http0x ->
                    rustTemplate(
                        """
                        let $bodyVarName = #{Hyper}::body::to_bytes($responseVarName.into_body()).await.expect("unable to extract body to bytes");
                        """,
                        "Hyper" to RuntimeType.Hyper,
                    )
            }
        }

    /**
     * Creates a writable that generates the proper HTTP request builder code.
     * Returns a string expression that creates a request with the given body.
     */
    fun createHttpRequest(
        codegenContext: ServerCodegenContext,
        uri: String,
        method: String,
        headers: Map<String, String>,
        bodyVariable: String,
    ): Writable =
        writable {
            val httpModule =
                if (codegenContext.runtimeConfig.httpVersion == HttpVersion.Http1x) {
                    CargoDependency.Http1x.toType()
                } else {
                    CargoDependency.Http.toType()
                }
            rustTemplate(
                """
            #{Http}::Request::builder()
                .uri($uri)
                .method($method)
                #{Headers:W}
                .body(#{Body:W})
                .expect("failed to build request")
            """,
                "Http" to httpModule,
                "Headers" to
                    writable {
                        headers.forEach { (name, value) ->
                            rust(".header(${name.dq()}, ${value.dq()})")
                        }
                    },
                "Body" to createBodyFromBytes(codegenContext, bodyVariable),
            )
        }
}

/**
 * Extension function to get HTTP dependencies scope with correct types.
 */
fun ServerCodegenContext.getHttpTestScope(): Array<Pair<String, RuntimeType>> {
    return ServerHttpTestHelpers.getHttpRuntimeTypeScope(this)
}

private fun String.dq() = "\"$this\""
