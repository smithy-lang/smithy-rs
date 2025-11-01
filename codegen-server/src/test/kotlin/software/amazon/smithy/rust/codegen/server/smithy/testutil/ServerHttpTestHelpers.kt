/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
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
        val httpDeps = codegenContext.httpDependencies()
        return arrayOf(
            "Http" to httpDeps.httpModule(),
            "Hyper" to RuntimeType.Hyper, // Note: Hyper has different APIs but same import
            "Tower" to RuntimeType.Tower,
            *RuntimeType.preludeScope
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
    fun createBodyFromBytes(codegenContext: ServerCodegenContext, bytesVariable: String): Writable = writable {
        if (codegenContext.isHttp1()) {
            rustTemplate(
                """#{HttpBodyUtilFull}::new(#{Bytes}::from($bytesVariable))""",
                "HttpBodyUtilFull" to codegenContext.httpDependencies().httpBodyUtil!!.toType().resolve("Full"),
                "Bytes" to RuntimeType.Bytes
            )
        } else {
            rustTemplate(
                """#{SmithyHttpServerBody}::from($bytesVariable)""",
                "SmithyHttpServerBody" to codegenContext.httpDependencies().smithyHttpServer.toType().resolve("body").resolve("Body")
            )
        }
    }

    /**
     * Creates a writable that generates code to read all bytes from an HTTP response body.
     *
     * For HTTP 0.x: `hyper::body::to_bytes(body).await`
     * For HTTP 1.x: `http_body_util::BodyExt::collect(body).await?.to_bytes()`
     */
    fun readBodyBytes(codegenContext: ServerCodegenContext, bodyExpr: String): Writable = writable {
        if (codegenContext.isHttp1()) {
            rustTemplate(
                """
                {
                    use #{BodyExt};
                    $bodyExpr.collect().await.expect("failed to read body").to_bytes()
                }
                """,
                "BodyExt" to codegenContext.httpDependencies().httpBodyUtil!!.toType().resolve("BodyExt")
            )
        } else {
            rustTemplate(
                """#{Hyper}::body::to_bytes($bodyExpr).await.expect("failed to read body")""",
                "Hyper" to RuntimeType.Hyper
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
        bodyVariable: String
    ): Writable = writable {
        val httpDeps = codegenContext.httpDependencies()
        rustTemplate(
            """
            #{Http}::Request::builder()
                .uri($uri)
                .method($method)
                #{Headers:W}
                .body(#{Body:W})
                .expect("failed to build request")
            """,
            "Http" to httpDeps.httpModule(),
            "Headers" to writable {
                headers.forEach { (name, value) ->
                    rust(".header(${name.dq()}, ${value.dq()})")
                }
            },
            "Body" to createBodyFromBytes(codegenContext, bodyVariable)
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
