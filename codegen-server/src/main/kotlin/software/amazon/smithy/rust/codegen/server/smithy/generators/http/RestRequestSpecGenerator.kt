/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

/**
 * [RestRequestSpecGenerator] generates a restJson1 or restXml specific `RequestSpec`. Both protocols are routed the same.
 */
class RestRequestSpecGenerator(
    private val httpBindingResolver: HttpBindingResolver,
    private val requestSpecModule: RuntimeType,
    private val runtimeConfig: RuntimeConfig,
    private val defaultRequestContentType: String,
) {
    fun generate(operationShape: OperationShape): Writable {
        val httpTrait = httpBindingResolver.httpTrait(operationShape)
        val extraCodegenScope =
            arrayOf(
                "RequestSpec",
                "UriSpec",
                "PathAndQuerySpec",
                "PathSpec",
                "QuerySpec",
                "PathSegment",
                "QuerySegment",
            ).map {
                it to requestSpecModule.resolve(it)
            }.toTypedArray() +
                arrayOf(
                    "RestRouteSpec" to restRouterModule().resolve("RestRouteSpec"),
                    "RequestContentType" to restRouterModule().resolve("RequestContentType"),
                )

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/950): Support the `endpoint` trait.
        val pathSegmentsVec =
            writable {
                withBlock("vec![", "]") {
                    for (segment in httpTrait.uri.segments) {
                        val variant =
                            when {
                                segment.isGreedyLabel -> "Greedy"
                                segment.isLabel -> "Label"
                                else -> """Literal(String::from("${segment.content}"))"""
                            }
                        rustTemplate(
                            "#{PathSegment}::$variant,",
                            *extraCodegenScope,
                        )
                    }
                }
            }

        val querySegmentsVec =
            writable {
                withBlock("vec![", "]") {
                    for (queryLiteral in httpTrait.uri.queryLiterals) {
                        val variant =
                            if (queryLiteral.value == "") {
                                """Key(String::from("${queryLiteral.key}"))"""
                            } else {
                                """KeyValue(String::from("${queryLiteral.key}"), String::from("${queryLiteral.value}"))"""
                            }
                        rustTemplate("#{QuerySegment}::$variant,", *extraCodegenScope)
                    }
                }
            }

        val requestContentType = requestContentTypeClaim(operationShape)

        return writable {
            rustTemplate(
                """
                #{RestRouteSpec}::new(
                    #{RequestSpec}::new(
                        #{Method}::${httpTrait.method},
                        #{UriSpec}::new(
                            #{PathAndQuerySpec}::new(
                                #{PathSpec}::from_vector_unchecked(#{PathSegmentsVec:W}),
                                #{QuerySpec}::from_vector_unchecked(#{QuerySegmentsVec:W})
                            )
                        )
                    ),
                    #{RequestContentType}::$requestContentType
                )
                """,
                *extraCodegenScope,
                "PathSegmentsVec" to pathSegmentsVec,
                "QuerySegmentsVec" to querySegmentsVec,
                "Method" to RuntimeType.http(runtimeConfig).resolve("Method"),
            )
        }
    }

    private fun requestContentTypeClaim(operationShape: OperationShape): String {
        val defaultContentType = httpBindingResolver.requestContentType(operationShape)
            ?: defaultRequestContentType
        val hasContentTypeHeaderBinding =
            httpBindingResolver.requestBindings(operationShape).any {
                it.location == HttpLocation.HEADER && it.locationName.equals("content-type", ignoreCase = true)
            }
        return if (hasContentTypeHeaderBinding) {
            "AnyValidContentType { default: ${defaultContentType.dq()} }"
        } else {
            "Expected(${defaultContentType.dq()})"
        }
    }

    private fun restRouterModule(): RuntimeType =
        ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
            .resolve("protocol::rest::router")
}
